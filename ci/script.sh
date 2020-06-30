# This script takes care of testing your crate

set -ex

# TODO This is the "test phase", tweak it as you see fit
main() {

    export CI_FREEBSD_HEADERS=/usr/include/freebsd
    cd wishbone-tool
    cross build --verbose --target $TARGET
    cross build --verbose --target $TARGET --release

    if [ ! -z $DISABLE_TESTS ]; then
        return
    fi

    cross test --target $TARGET
    cross test --target $TARGET --release

    # Don't run the program, since it needs hardware attached.
    # cross run --target $TARGET
    # cross run --target $TARGET --release
}

# we don't run the "test phase" when doing deploys
if [ -z $TRAVIS_TAG ]; then
    main
fi
