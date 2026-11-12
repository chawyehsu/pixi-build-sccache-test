# pixi-build-sccache-test

A test project for building with pixi build and using sccache for caching.

## Problem

When building the project with pixi build, the build sometimes fails with a
weird sccache error on Windows.

## Diagnosis

See the following two [builds](https://github.com/chawyehsu/pixi-build-sccache-test/actions/workflows/build.yml):

- The one that failed: https://github.com/chawyehsu/pixi-build-sccache-test/actions/runs/43151377693/job/130961075089
- The one that succeeded: https://github.com/chawyehsu/pixi-build-sccache-test/actions/runs/43151465544/job/130961380325

The only difference between the two builds is whether the pre-step was run before the actual build.

https://github.com/chawyehsu/pixi-build-sccache-test/blob/main/.github/workflows/build.yml#L59-L62

The pre-step is defined as follows in the workflow file:

```yaml
      - name: Test
        if: github.event.inputs.prestep == 'yes'
        run: pixi run -e build cargo run
```

The pixi build backend uses an isolated environment for building and should not
requires the extra pre-step to work correctly. However, without the pre-step,
the build fails due to a weird sccache error of:

```
thread 'main' (2104) panicked at src\config.rs:1300:55:
 Unable to get config directory
```

From sccache's codebase, the error originates from the following line in `src/config.rs`:

https://github.com/mozilla/sccache/blob/v0.19.0/src/config.rs#L1300

It seems that sccache is unable to determine the configuration directory, which
is theoretically impossible under normal circumstances, as it should always be able
to locate the appropriate directory from the underlying system call.
