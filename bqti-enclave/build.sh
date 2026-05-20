#!/bin/bash
set -e

SCRIPT_DIR=$(dirname $(realpath $0))
KEYSTONE_DIR=${KEYSTONE_DIR:-$HOME/development/thesis-testing/keystone}
RISCV_GCC=$KEYSTONE_DIR/build-generic64/buildroot.build/per-package/keystone-examples/host/bin/riscv64-buildroot-linux-gnu-gcc
SYSROOT=$KEYSTONE_DIR/build-generic64/buildroot.build/per-package/keystone-examples/host/riscv64-buildroot-linux-gnu/sysroot

mkdir -p build && cd build

cmake .. \
  -DCMAKE_TOOLCHAIN_FILE=$SCRIPT_DIR/vendor/keystone/toolchainfile.cmake \
  -DKEYSTONE_SDK_DIR=$SCRIPT_DIR/vendor/keystone/sdk \
  -DCMAKE_C_COMPILER=$RISCV_GCC \
  -DCMAKE_CXX_COMPILER=${RISCV_GCC/gcc/g++} \
  -DCMAKE_SYSTEM_NAME=Linux \
  -DCMAKE_SYSTEM_PROCESSOR=riscv64 \
  -DCMAKE_SYSROOT=$SYSROOT \
  -DKEYSTONE_RUNTIME=$HOME/development/thesis-testing/keystone/runtime \
  -DCMAKE_EXPORT_COMPILE_COMMANDS=ON \
  -DCMAKE_FIND_ROOT_PATH=$SYSROOT

make -j$(nproc)
