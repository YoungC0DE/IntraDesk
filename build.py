#!/usr/bin/env python3

import os
import contextlib
import platform
import shutil
import argparse
import sys

win_arch = 'arm64' if platform.machine().lower() in ('arm64', 'aarch64') else 'x64'
flutter_build_dir = f'build/windows/{win_arch}/runner/Release/'
flutter_build_dir_2 = f'flutter/{flutter_build_dir}'


def system2(cmd):
    exit_code = os.system(cmd)
    if exit_code != 0:
        sys.stderr.write(f"Error occurred when executing: `{cmd}`. Exiting.\n")
        sys.exit(-1)


def get_version():
    with open("Cargo.toml", encoding="utf-8") as fh:
        for line in fh:
            if line.startswith("version"):
                return line.replace("version", "").replace("=", "").replace('"', '').strip()
    return ''


def make_parser():
    parser = argparse.ArgumentParser(description='Build script (Windows only).')
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
        '--portable',
        action='store_true',
        help='Build windows portable'
    )
    parser.add_argument(
        '--print-features',
        action='store_true',
        help='Print the cargo feature list these flags select, and exit without building. For a '
             'caller that runs its own cargo line: it can ask for the list rather than repeat it, '
             'so the two cannot drift.'
    )
    parser.add_argument(
        '--skip-portable-pack',
        action='store_true',
        help='Skip packing'
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


def build_flutter_windows(version, features, skip_portable_pack):
    system2(f'cargo build --locked --features {features} --lib --release')
    if not os.path.exists("target/release/librustdesk.dll"):
        print("cargo build failed, please check rust source code.")
        exit(-1)
    os.chdir('flutter')
    system2('flutter build windows --release')
    os.chdir('..')
    shutil.copy2('target/release/deps/dylib_virtual_display.dll',
                 flutter_build_dir_2)
    if skip_portable_pack:
        return
    os.chdir('libs/portable')
    system2('pip3 install -r requirements.txt')
    system2(
        f'python3 ./generate.py -f ../../{flutter_build_dir_2} -o . -e ../../{flutter_build_dir_2}/IntraDesk.exe')
    os.chdir('../..')
    if os.path.exists('./rustdesk_portable.exe'):
        os.replace('./target/release/rustdesk-portable-packer.exe',
                   './rustdesk_portable.exe')
    else:
        os.rename('./target/release/rustdesk-portable-packer.exe',
                  './rustdesk_portable.exe')
    print(
        f'output location: {os.path.abspath(os.curdir)}/rustdesk_portable.exe')
    os.rename('./rustdesk_portable.exe', f'./rustdesk-{version}-install.exe')
    print(
        f'output location: {os.path.abspath(os.curdir)}/rustdesk-{version}-install.exe')


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
    version = get_version()
    features = ','.join(get_features(args))
    # build virtual display dynamic library
    os.chdir('libs/virtual_display/dylib')
    system2('cargo build --locked --release')
    os.chdir('../../..')

    build_flutter_windows(version, features, args.skip_portable_pack)


if __name__ == "__main__":
    main()
