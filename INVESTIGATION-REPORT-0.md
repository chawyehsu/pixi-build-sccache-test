# Investigation Report: `sccache` panic in `pixi build` on Windows

- **Date:** 2026-11-12
- **Model:** `deepseek-v4.1-flash`
- **Repository:** `chawyehsu/pixi-build-sccache-test`
- **Subject:** `thread 'main' panicked at src\config.rs:1300:55: Unable to get config directory`

---

## 1. Executive summary

The failure is **not** a random sccache bug and it is **not** caused by the isolated
build environment being unable to reach the host's cache. It is a deterministic
interaction between three things:

1. `rattler-build`'s **strict environment isolation** on Windows overrides
   `USERPROFILE` with the build's `work` directory.
2. The `work` directory is created empty for a path source (`No sources to fetch`),
   so `<work>\AppData\Roaming` and `<work>\AppData\Local` do not exist.
3. `sccache v0.19.0` uses `directories 6.0` → `dirs-sys 0.5.0`, whose Windows
   implementation calls `SHGetKnownFolderPath(FOLDERID_RoamingAppData)` and
   `SHGetKnownFolderPath(FOLDERID_LocalAppData)`. `SHGetKnownFolderPath` resolves
   `%USERPROFILE%\AppData\...` and **fails when the target directory does not
   exist**. `sccache` then calls `.expect("Unable to get config directory")` and
   panics.

The pre-step (`pixi run -e build cargo run`) works around the problem by causing
the build's redirected profile directories to exist before the real
`cargo install` runs (the successful run's sccache cache is literally located in
`...\work\AppData\Local\Mozilla\sccache\cache`). That makes the workaround
timing-dependent and fragile; it is not a real fix.

The correct fix is upstream: either `rattler-build` should create
`<work>\AppData\Roaming` and `<work>\AppData\Local` when it redirects
`USERPROFILE`, or `sccache` should not `expect()` on `ProjectDirs::from()` and
should fall back gracefully.

---

## 2. CI evidence

The README points at two runs:

- **Failed:** <https://github.com/chawyehsu/pixi-build-sccache-test/actions/runs/43151377693/job/130961075089>
- **Succeeded:** <https://github.com/chawyehsu/pixi-build-sccache-test/actions/runs/43151465544/job/130961380325>
- Workflow source: <https://github.com/chawyehsu/pixi-build-sccache-test/blob/main/.github/workflows/build.yml#L55-L62>

The only workflow difference is this optional step:

```yaml
      - name: Test
        if: github.event.inputs.prestep == 'yes'
        run: pixi run -e build cargo run
```

### 2.1 Failing run

The failure happens at the very first sccache invocation made by `cargo install`:

```text
%SRC_DIR%>"%BUILD_PREFIX%\\Library\\bin\\cargo" install --locked --root "%PREFIX%" \
  --path "D:\a\pixi-build-sccache-test\pixi-build-sccache-test" \
  --target-dir target --no-track  --force
error: process didn't exit successfully: `sccache %BUILD_PREFIX%\Library\bin\rustc -vV` (exit code: 101)
--- stderr
thread 'main' (2104) panicked at src\config.rs:1300:55:
Unable to get config directory
```

The build script sets `RUSTC_WRAPPER=sccache` and the panic comes from sccache's
own config loading, not from rustc.

### 2.2 Successful run

The pre-step prints the *host* profile paths, which are normal:

```text
Local App Data folder: "C:\\Users\\runneradmin\\AppData\\Local"
Roaming App Data folder: "C:\\Users\\runneradmin\\AppData\\Roaming"
```

The build then succeeds, and `sccache --show-stats` reports:

```text
Cache location  Local disk: "D:\\a\\...\\.pixi\\bld\\pixi-build-sccache-test\\IZa73cFgvTw\\work\\AppData\\Local\\Mozilla\\sccache\\cache"
```

That path is decisive: the build's sccache server ran with
`USERPROFILE=<work>` and successfully resolved
`<work>\AppData\Local`. Therefore, in the successful run, **both**
`<work>\AppData\Local` and `<work>\AppData\Roaming` existed at the time
sccache loaded its config.

---

## 3. Code-path analysis

### 3.1 sccache v0.19.0

The panic site is exactly the README's line:

- <https://github.com/mozilla/sccache/blob/v0.19.0/src/config.rs#L1295-L1300>

```rust
fn config_file(env_var: &str, leaf: &str) -> PathBuf {
    if let Some(env_value) = env::var_os(env_var) {
        return env_value.into();
    }
    let dirs =
        ProjectDirs::from("", ORGANIZATION, APP_NAME).expect("Unable to get config directory");
    // ...
}
```

`Config::load()` calls `config_file("SCCACHE_CONF", "config")` **unconditionally**,
and `run_command()` calls `Config::load()` before doing anything else:

- <https://github.com/mozilla/sccache/blob/v0.19.0/src/commands.rs#L797-L801>

```rust
pub fn run_command(cmd: Command) -> Result<i32> {
    // Config isn't required for all commands, but if it's broken then we should flag
    // it early and loudly.
    let config = &Config::load()?;
```

This matters: **an already-running sccache server does not avoid the panic**,
because the client still loads config before it tries to connect.

The same `ProjectDirs::from(...).expect(...)` pattern also appears in:

- `default_disk_cache_dir()` — <https://github.com/mozilla/sccache/blob/v0.19.0/src/config.rs#L102-L107>
- `default_dist_cache_dir()` — <https://github.com/mozilla/sccache/blob/v0.19.0/src/config.rs#L109-L114>

`SCCACHE_DIR` only avoids `default_disk_cache_dir()` (the "any_overridden" branch
at <https://github.com/mozilla/sccache/blob/v0.19.0/src/config.rs#L1204-L1234>);
it does **not** avoid `config_file()`. Setting `SCCACHE_CONF` avoids
`config_file()`, but `DistConfig::default()` still calls
`default_dist_cache_dir()`, so the panic simply moves to `src/config.rs:111`
(`Unable to retrieve dist cache directory`). This was confirmed empirically.

sccache v0.19.0 depends on `directories = "6.0"`:

- <https://github.com/mozilla/sccache/blob/v0.19.0/Cargo.toml>

### 3.2 `directories` / `dirs-sys` on Windows

`directories 6.0.0`'s Windows backend uses the Known Folder API:

- <https://docs.rs/crate/directories/6.0.0/source/src/win.rs>

```rust
pub fn project_dirs_from_path(project_path: PathBuf) -> Option<ProjectDirs> {
    let app_data_local   = dirs_sys::known_folder_local_app_data();
    let app_data_roaming = dirs_sys::known_folder_roaming_app_data();
    if let (Some(app_data_local), Some(app_data_roaming)) = (app_data_local, app_data_roaming) {
        // ...
    } else {
        None
    }
}
```

`dirs-sys 0.5.0` (the exact version also used by this repo's `src/main.rs`)
calls `SHGetKnownFolderPath` and returns `None` on any non-zero result:

- <https://docs.rs/crate/dirs-sys/0.5.0/source/src/lib.rs>

```rust
pub fn known_folder(folder_id: windows::core::GUID) -> Option<PathBuf> {
    unsafe {
        let mut path_ptr: windows::core::PWSTR = std::ptr::null_mut();
        let result = Shell::SHGetKnownFolderPath(
            &folder_id,
            0,
            std::ptr::null_mut(),
            &mut path_ptr,
        );
        if result == 0 {
            // ...
            Some(PathBuf::from(ostr))
        } else {
            // ...
            None
        }
    }
}
```

So `ProjectDirs::from(...)` returns `None` as soon as **either**
`LocalAppData` or `RoamingAppData` cannot be resolved, and sccache's `.expect()`
turns that into a panic.

### 3.3 `SHGetKnownFolderPath` behavior

Microsoft documents that the function returns an error if the known folder
cannot be retrieved; it does not create the folder:

- <https://learn.microsoft.com/en-us/windows/win32/api/shlobj_core/nf-shlobj_core-shgetknownfolderpath>

The practical behavior on Windows (confirmed empirically below) is that the
shell resolves `FOLDERID_RoamingAppData` / `FOLDERID_LocalAppData` through the
current `USERPROFILE`, and the lookup fails when the corresponding
`AppData\Roaming` / `AppData\Local` directory does not exist.

### 3.4 rattler-build strict isolation

`rattler-build` is the build executor behind the `pixi-build-rust` backend.
Its `os_vars()` function does this for Windows builds:

- <https://github.com/prefix-dev/rattler-build/blob/main/crates/rattler_build_core/src/env_vars.rs>

```rust
if build_platform.is_windows() {
    match env_isolation {
        EnvironmentIsolation::Strict => {
            insert!(vars, "USERPROFILE", work_dir.to_string_lossy());
        }
        // ...
    }
}
```

Meanwhile `default_env_vars_build()` forwards `APPDATA` and `LOCALAPPDATA` from
the parent environment:

- <https://github.com/prefix-dev/rattler-build/blob/main/crates/rattler_build_core/src/windows/env.rs>

So inside the build script:

- `USERPROFILE` = `<work>` (the empty work dir)
- `APPDATA` = `C:\Users\<host>\AppData\Roaming`
- `LOCALAPPDATA` = `C:\Users\<host>\AppData\Local`

But `SHGetKnownFolderPath` does not use `APPDATA` / `LOCALAPPDATA` for these
folders; it expands the profile path from `USERPROFILE`, which is now the empty
work dir. That is the root cause.

The environment isolation behavior is also described in the build-script docs:

- <https://rattler-build.prefix.dev/latest/build_script/>

### 3.5 Why the backend's sccache detection is not the cause

The `pixi-build-rust` backend sets `RUSTC_WRAPPER=sccache` when any `SCCACHE*`
environment variable is present (this repo uses `SCCACHE_ENABLED` as a trigger):

- <https://github.com/prefix-dev/pixi-build-backends/blob/main/crates/pixi-build-rust/src/build_script.j2>
- <https://github.com/prefix-dev/pixi-build-backends/blob/main/crates/pixi-build-rust/src/main.rs>
- <https://github.com/prefix-dev/pixi-build-backends/blob/main/crates/pixi-build-backend/src/cache.rs>

That is a separate detection quirk (see also
<https://github.com/prefix-dev/pixi/issues/7510>), but it only decides *whether*
sccache is used. The panic itself is the `USERPROFILE` / Known Folder issue.

---

## 4. Local reproduction

The reproduction was done with the exact conda-forge artifact referenced by this
repo's `pixi.lock`:

```text
https://prefix.dev/conda-forge/win-64/sccache-0.19.0-h18a1a76_0.conda
```

and independently with the GitHub release binary
`sccache-v0.19.0-x86_64-pc-windows-msvc.zip`.

### 4.1 Direct sccache panic

```powershell
$work = Join-Path $env:TEMP ("work-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $work | Out-Null

# Match CI: GHA backend enabled, no SCCACHE_CACHE_SIZE override.
Remove-Item Env:\SCCACHE_CACHE_SIZE -ErrorAction SilentlyContinue
$env:SCCACHE_GHA_ENABLED = "true"
$env:ACTIONS_CACHE_SERVICE_V2 = "on"
$env:USERPROFILE = $work

& sccache.exe (Get-Command rustc).Source -vV
```

Result:

```text
thread 'main' (… ) panicked at src\config.rs:1300:55:
Unable to get config directory
```

This is byte-for-byte the CI error.

If `SCCACHE_CACHE_SIZE` is set (as it is on the investigation machine), the
panic happens earlier, in `default_disk_cache_dir()`:

```text
thread 'main' (… ) panicked at src\config.rs:104:10:
Unable to retrieve disk cache directory
```

Both are the same underlying `ProjectDirs::from()` failure.

### 4.2 Directory requirements

Using this repo's own `dirs-sys 0.5.0` probe (`src/main.rs`) with a redirected
`USERPROFILE`:

| `<work>\AppData` state | Local AppData | Roaming AppData |
|---|---|---|
| absent | fail (`-2147024893` / `0x80070003`) | fail (`-2147024893`) |
| `Roaming` only | fail (`-2147024894` / `0x80070002`) | OK |
| `Roaming` + `Local` | OK | OK |

Conclusion: `ProjectDirs::from()` needs **both** `<USERPROFILE>\AppData\Roaming`
and `<USERPROFILE>\AppData\Local` to exist.

### 4.3 Environment-variable workaround matrix

With `USERPROFILE` pointing at an empty dir and GHA mode enabled:

| Configuration | Result |
|---|---|
| baseline | panic at `config.rs:1300` (`Unable to get config directory`) |
| `SCCACHE_DIR` set | panic at `config.rs:1300` (does not help) |
| `SCCACHE_CONF` set | panic at `config.rs:111` (`Unable to retrieve dist cache directory`) |
| `SCCACHE_CONF` + `SCCACHE_DIR` | panic at `config.rs:111` (does not help) |
| `AppData\Roaming` + `AppData\Local` created | config loads; only the expected GHA-cache error remains outside Actions |

This is why the `SCCACHE_DIR` note in `pixi.toml` is not sufficient by itself:
`config_file()` and `DistConfig::default()` run before any cache-dir override is
applied.

### 4.4 Why the pre-step changes the outcome

The pre-step itself runs **outside** the build sandbox and cannot create the
sandbox's `<work>\AppData` directories. What it does do is warm up the host-side
toolchain / sccache server so that the build's own startup side effects
(the `vs2022` activation script and its `vcvars64.bat` → PowerShell first-run,
plus `sccache --show-stats` in the generated build script) end up creating the
redirected profile directories before the real `cargo install` probe runs.

The successful run proves this happened: its sccache cache is under
`<work>\AppData\Local\Mozilla\sccache\cache`. Locally, the same `pixi build`
succeeds without a pre-step because the `vs2022` activation script creates
`<work>\AppData\Local\Microsoft\PowerShell\...` and the associated PowerShell
startup creates `<work>\AppData\Roaming`. Removing the shell AutoRun hook did
not prevent this (PowerShell, not Clink, was the creator).

This makes the pre-step a **timing/ordering workaround**, not a fix. If the
backend's script stops running `sccache --show-stats`, or the activation path
changes, the panic returns.

---

## 5. Related upstream reports

- `water-rs/waterui#3124` — same failure mode: Windows tests redirect
  `USERPROFILE`, `RUSTC_WRAPPER=sccache` panics with
  `Unable to get config directory`:
  <https://github.com/water-rs/waterui/issues/3124>
- `mozilla/sccache#939` — older report of the same `config_file` panic on
  Windows: <https://github.com/mozilla/sccache/issues/939>
- `prefix-dev/rattler-build#2824` — strict isolation drops
  `sccache`/`ccache`-related environment variables by default:
  <https://github.com/prefix-dev/rattler-build/issues/2824>
- `prefix-dev/pixi#7510` — `pixi-build-rust` sccache auto-detection ignores
  system envs in some cases:
  <https://github.com/prefix-dev/pixi/issues/7510>
- `prefix-dev/pixi#7437` — sccache GHA secrets are not captured/passed:
  <https://github.com/prefix-dev/pixi/issues/7437>

---

## 6. Recommended fixes

### 6.1 Upstream (preferred)

1. **`rattler-build`**: when applying `EnvironmentIsolation::Strict` on Windows,
   create `<work>\AppData\Roaming` and `<work>\AppData\Local` before spawning the
   build script, or avoid overriding `USERPROFILE` and instead isolate via
   `HOME`/`APPDATA`/`LOCALAPPDATA` in a way that still leaves a valid profile.
2. **`sccache`**: replace the `.expect()` calls in `config_file()`,
   `default_disk_cache_dir()`, and `default_dist_cache_dir()` with a graceful
   fallback (for example, `USERPROFILE\AppData\Roaming` / `Local`), so a fresh
   redirected profile does not crash the compiler wrapper.

### 6.2 Repository-level

- Keep the pre-step for now if CI stability is the priority, but treat it as a
  workaround.
- A more direct workaround is to ensure the profile directories exist at the
  start of the build. For example, a build-script prelude (or a wrapper around
  `cargo install`) that runs:
  ```bat
  if not exist "%USERPROFILE%\AppData\Roaming" mkdir "%USERPROFILE%\AppData\Roaming"
  if not exist "%USERPROFILE%\AppData\Local"   mkdir "%USERPROFILE%\AppData\Local"
  ```
  This addresses the actual condition instead of relying on a pre-step.
- `SCCACHE_DIR` alone is **not** a workaround (verified). `SCCACHE_CONF` alone
  is **not** a workaround either, because of `DistConfig::default()`.
- Consider testing whether setting `USERPROFILE` in
  `[package.build.config].env` (or the backend's script env) takes precedence
  over `rattler-build`'s strict override; if it does, pointing it at a prepared
  profile directory would also work. This precedence was not verified here.

---

## 7. Exact references

| Topic | Link |
|---|---|
| sccache v0.19.0 panic site | <https://github.com/mozilla/sccache/blob/v0.19.0/src/config.rs#L1300> |
| sccache default cache dirs | <https://github.com/mozilla/sccache/blob/v0.19.0/src/config.rs#L102-L114> |
| sccache disk-dir env handling | <https://github.com/mozilla/sccache/blob/v0.19.0/src/config.rs#L1204-L1234> |
| sccache unconditional `Config::load` | <https://github.com/mozilla/sccache/blob/v0.19.0/src/commands.rs#L797-L801> |
| sccache `directories = "6.0"` | <https://github.com/mozilla/sccache/blob/v0.19.0/Cargo.toml> |
| `directories 6.0.0` Windows backend | <https://docs.rs/crate/directories/6.0.0/source/src/win.rs> |
| `dirs-sys 0.5.0` Known Folder wrapper | <https://docs.rs/crate/dirs-sys/0.5.0/source/src/lib.rs> |
| `SHGetKnownFolderPath` docs | <https://learn.microsoft.com/en-us/windows/win32/api/shlobj_core/nf-shlobj_core-shgetknownfolderpath> |
| rattler-build `USERPROFILE` override | <https://github.com/prefix-dev/rattler-build/blob/main/crates/rattler_build_core/src/env_vars.rs> |
| rattler-build Windows env forwarding | <https://github.com/prefix-dev/rattler-build/blob/main/crates/rattler_build_core/src/windows/env.rs> |
| rattler-build build-script docs | <https://rattler-build.prefix.dev/latest/build_script/> |
| pixi-build-rust sccache script | <https://github.com/prefix-dev/pixi-build-backends/blob/main/crates/pixi-build-rust/src/build_script.j2> |
| Failed CI job | <https://github.com/chawyehsu/pixi-build-sccache-test/actions/runs/43151377693/job/130961075089> |
| Successful CI job | <https://github.com/chawyehsu/pixi-build-sccache-test/actions/runs/43151465544/job/130961380325> |
| Workflow pre-step | <https://github.com/chawyehsu/pixi-build-sccache-test/blob/main/.github/workflows/build.yml#L55-L62> |

---

## 8. Bottom line

The README's instinct is correct: the build should not need the pre-step. The
real defect is that `rattler-build` gives the build script a `USERPROFILE` that
does not have the standard `AppData` subdirectories, while `sccache` assumes
`ProjectDirs::from()` can never fail and panics when it does. Either side can
fix it; until then, the pre-step only hides the race.
