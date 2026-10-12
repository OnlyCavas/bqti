WGET_VERSION := $(shell wget --version 2>/dev/null | head -1)
ifneq ($(findstring Wget2,$(WGET_VERSION)),)
  KEYSTONE_ARGS += BR2_WGET="wget -nd -t 3"
endif

KEYSTONE_DIR :=	vendor/keystone
KS_HOST :=	$(abspath $(KEYSTONE_DIR)/build-generic64/buildroot.build/host)
RISCV_GCC := $(KS_HOST)/bin/riscv64-buildroot-linux-gnu-gcc
RISCV_SYSROOT := $(KS_HOST)/riscv64-buildroot-linux-gnu/sysroot/

KEYSTONE_HOST_CFLAGS := -O2 -std=gnu17 \
  -Wno-error=implicit-function-declaration \
  -Wno-error=implicit-int \
  -Wno-error=int-conversion \
  -Wno-error=incompatible-pointer-types

VM_PORT  ?= 9821
SSH_OPTS := -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null
ENCLAVE_DIR := $(CURDIR)/bqti-enclave
ENCLAVE_KE := $(ENCLAVE_DIR)/build/bqti.ke
BQTI_BIN := $(CURDIR)/target/riscv64gc-unknown-linux-gnu/release/bqti

define CARGO_CONFIG
[target.riscv64gc-unknown-linux-gnu]
linker = "$(RISCV_GCC)"

[env]
RISCV_GCC = "$(RISCV_GCC)"
RISCV_SYSROOT = "$(RISCV_SYSROOT)"
endef
export CARGO_CONFIG

.DEFAULT_GOAL := run
.PHONY: keystone cargo-config compile-enclave compile-bqti deploy-enclave deploy-bqti start-vm run

$(KEYSTONE_DIR)/.git:
	git submodule update --init --recursive --depth 1

$(KEYSTONE_DIR)/.build : $(KEYSTONE_DIR)/.git
	HOST_CFLAGS="$(KEYSTONE_HOST_CFLAGS)" $(MAKE) -C $(KEYSTONE_DIR) $(KEYSTONE_ARGS)
	touch $@

keystone: $(KEYSTONE_DIR)/.build

.cargo/config.toml: Makefile $(KEYSTONE_DIR)/.build
	mkdir -p .cargo
	printf '%s\n' "$$CARGO_CONFIG" > $@

cargo-config: .cargo/config.toml

compile-enclave:
	$(MAKE) -C $(ENCLAVE_DIR)

deploy-enclave: compile-enclave
	sshpass -p sifive scp -O $(SSH_OPTS) -P $(VM_PORT) \
		$(ENCLAVE_KE) root@localhost:/root/

compile-bqti:
	cargo build --target riscv64gc-unknown-linux-gnu

deploy-bqti: compile-bqti
	sshpass -p sifive scp -O $(SSH_OPTS) -P $(VM_PORT) \
		$(BQTI_BIN) root@localhost:/root/

start-vm: keystone
	expect -c '\
		set timeout -1; \
		spawn $(MAKE) -C $(KEYSTONE_DIR) run; \
		expect "login:"; send "root\r"; \
		expect "Password:"; send "sifive\r"; \
		expect "# "; \
		send "modprobe keystone-driver\r"; \
		expect "# "; \
		interact'

run: deploy-enclave deploy-bqti
