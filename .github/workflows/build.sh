set -ex

main() {
    local release_flag=""
    if [ "$IS_DEPLOY" = "true" ]; then
        release_flag="--profile max-opt"
    fi

    if [ -z "$FEATURES" ]; then
        FEATURE_FLAGS="--no-default-features"
    else
        FEATURE_FLAGS="--no-default-features --features $FEATURES"
    fi

    cargo build --locked --target "$TARGET" $release_flag $FEATURE_FLAGS
}

main
