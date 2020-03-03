#!/bin/bash

set -euxo pipefail

main() {
    local pkg_name=cortex-m-udf

    rm -f bin/*.a

    arm-none-eabi-as -march=armv6s-m asm.s -o bin/$pkg_name.o
    ar crs bin/thumbv6m-none-eabi.a bin/$pkg_name.o

    arm-none-eabi-as -march=armv7-m asm.s -o bin/$pkg_name.o
    ar crs bin/thumbv7m-none-eabi.a bin/$pkg_name.o

    arm-none-eabi-as -march=armv7e-m asm.s -o bin/$pkg_name.o
    ar crs bin/thumbv7em-none-eabi.a bin/$pkg_name.o
    ar crs bin/thumbv7em-none-eabihf.a bin/$pkg_name.o

    rm bin/*.o
}

main
