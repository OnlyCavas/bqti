VENDOR_DIR       := $(CURDIR)/vendor
KEYSTONE_DIR     ?= $(abspath $(CURDIR)/../vendor/keystone)
BUILDROOT 			 := $(KEYSTONE_DIR)/build-generic64/buildroot.build

TOOLCHAIN_FILE   ?= $(VENDOR_DIR)/keystone/toolchainfile.cmake
KEYSTONE_SDK_DIR ?= $(VENDOR_DIR)/keystone/sdk
RISCV_GCC        ?= $(BUILDROOT)/host/bin/riscv64-buildroot-linux-gnu-gcc
RISCV_GXX        ?= $(RISCV_GCC:%gcc=%g++)
SYSROOT          ?= $(BUILDROOT)/host/riscv64-buildroot-linux-gnu/sysroot
KEYSTONE_RUNTIME ?= $(KEYSTONE_DIR)/runtime

BUILD_DIR        ?= $(CURDIR)/build
JOBS             ?= $(shell nproc)
TARGET           ?= examples

.PHONY: all configure check clean
run: compile-json
	cmake --build "$(BUILD_DIR)" --target examples -j$(JOBS)

compile-json: configure
	ln -sf "$(BUILD_DIR)/compile_commands.json" "$(CURDIR)/compile_commands.json"

configure: check clean
	cmake -S . -B "$(BUILD_DIR)" \
	  -DCMAKE_TOOLCHAIN_FILE="$(TOOLCHAIN_FILE)" \
	  -DKEYSTONE_SDK_DIR="$(KEYSTONE_SDK_DIR)" \
	  -DKEYSTONE_RUNTIME="$(KEYSTONE_RUNTIME)" \
	  -DCMAKE_C_COMPILER="$(RISCV_GCC)" \
	  -DCMAKE_CXX_COMPILER="$(RISCV_GXX)" \
	  -DCMAKE_SYSROOT="$(SYSROOT)" \
	  -DCMAKE_FIND_ROOT_PATH="$(SYSROOT)" \
	  -DCMAKE_EXPORT_COMPILE_COMMANDS=ON \
		-DCMAKE_SYSTEM_PROCESSOR=riscv64 \
		-DCMAKE_SYSTEM_NAME=Linux

check:
	@test -f "$(TOOLCHAIN_FILE)" || \
	  { echo "Missing $(TOOLCHAIN_FILE): build Keystone first"; exit 1; }
	@test -f "$(KEYSTONE_SDK_DIR)/cmake/macros.cmake" || \
	  { echo "Missing $(KEYSTONE_SDK_DIR)/cmake/macros.cmake"; exit 1; }

clean:
	rm -rf "$(BUILD_DIR)"
	rm -rf "$(CURDIR)/compile_commands.json"
