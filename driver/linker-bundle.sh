#!/bin/bash
# Linker wrapper that produces MH_BUNDLE instead of MH_DYLIB.
#
# Rust's `cdylib` crate-type passes `-dynamiclib` to the linker, producing
# MH_DYLIB. CoreAudio HAL plugins require MH_BUNDLE. This wrapper intercepts
# the linker invocation and replaces `-dynamiclib` with `-bundle`.
#
# Used via .cargo/config.toml: linker = "linker-bundle.sh"

args=()
for arg in "$@"; do
    if [ "$arg" = "-dynamiclib" ]; then
        args+=("-bundle")
    else
        args+=("$arg")
    fi
done

exec cc "${args[@]}"
