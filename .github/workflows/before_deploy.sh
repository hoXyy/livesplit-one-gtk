set -ex

main() {
    local tag=$(git tag --points-at HEAD)
    local src=$(pwd)
    local stage=$(mktemp -d)

    cp "target/$TARGET/max-opt/livesplit-one-gtk" "$stage/livesplit-one-gtk"
    tar -C "$stage" -czf "$src/livesplit-one-$tag-$RELEASE_TARGET.tar.gz" livesplit-one-gtk
    rm -rf "$stage"
}

main
