#!/usr/bin/env python3

import os
import contextlib
import platform
import argparse
import sys

def system2(cmd):
    exit_code = os.system(cmd)
    if exit_code != 0:
        sys.stderr.write(f"Error occurred when executing: `{cmd}`. Exiting.\n")
        sys.exit(-1)


def make_parser():
    parser = argparse.ArgumentParser(
        description='Builds the Rust library and the Flutter app (Windows only). '
                    'scripts/build.ps1 and the CI then pack them into one IntraDesk.exe.')
    parser.add_argument(
        '--hwcodec',
        action='store_true',
        help='Enable feature hwcodec'
    )
    parser.add_argument(
        '--vram',
        action='store_true',
        help='Enable feature vram'
    )
    parser.add_argument(
        '--print-features',
        action='store_true',
        help='Print the cargo feature list these flags select, and exit without building. For a '
             'caller that runs its own cargo line: it can ask for the list rather than repeat it, '
             'so the two cannot drift.'
    )
    return parser


def get_features(args):
    features = []
    if args.hwcodec:
        features.append('hwcodec')
    if args.vram:
        features.append('vram')
    print("features:", features)
    return features


def build_flutter_windows(features):
    system2(f'cargo build --locked --features {features} --lib --release')
    if not os.path.exists("target/release/librustdesk.dll"):
        print("cargo build failed, please check rust source code.")
        exit(-1)
    os.chdir('flutter')
    system2('flutter build windows --release')
    os.chdir('..')


def main():
    parser = make_parser()
    args = parser.parse_args()

    # Before anything with a side effect: this is a query, and a caller uses it to build the very
    # binary it will then package. `get_features` stays the single definition of what a flag
    # combination means; a caller that hardcodes the list instead is one edit away from compiling
    # something other than what it ships.
    if args.print_features:
        # stdout carries the list and nothing else, so a caller can use it directly in a command
        # substitution. `get_features` prints a human-readable line of its own; send that to stderr
        # for this call rather than silencing it, which would change what every other path prints.
        with contextlib.redirect_stdout(sys.stderr):
            feats = ','.join(get_features(args))
        print(feats)
        return

    if not platform.platform().startswith('Windows'):
        sys.exit('IntraDesk builds on Windows only.')
    features = ','.join(get_features(args))
    build_flutter_windows(features)


if __name__ == "__main__":
    main()
