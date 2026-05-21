include(${KEYSTONE_SDK_DIR}/cmake/macros.cmake)

cmake_minimum_required(VERSION 3.10)
project(bqti-enclave)
add_custom_target(examples)

set(eapp_bin bqti)
set(eapp_src src/eapp/bqti.c)

set(host_bin bqti-runner)
set(host_src src/host/bqti-runner.cpp)
set(package_name "bqti.ke")
set(package_script "./bqti-runner bqti eyrie-rt loader.bin")
set(eyrie_plugins "io_syscall linux_syscall env_setup")

set(KEYSTONE_LIB_EAPP ${KEYSTONE_SDK_DIR}/lib/libkeystone-eapp.a)
set(KEYSTONE_LIB_HOST ${KEYSTONE_SDK_DIR}/lib/libkeystone-host.a)
set(KEYSTONE_LIB_EDGE ${KEYSTONE_SDK_DIR}/lib/libkeystone-edge.a)

# mbedtls (SHA-256 only)
set(ENABLE_TESTING  OFF CACHE BOOL "" FORCE)
set(ENABLE_PROGRAMS OFF CACHE BOOL "" FORCE)
add_subdirectory(vendor/mbedtls)

# monocypher
add_library(monocypher STATIC
  vendor/monocypher/monocypher.c
  vendor/monocypher/monocypher-ed25519.c)
target_include_directories(monocypher PUBLIC vendor/monocypher)
target_compile_options(monocypher PRIVATE -ffunction-sections -fdata-sections)

# eapp
add_executable(${eapp_bin} ${eapp_src})
target_link_libraries(${eapp_bin} ${KEYSTONE_LIB_EAPP} tfpsacrypto monocypher "-static" "-Wl,--gc-sections")
target_include_directories(${eapp_bin} PUBLIC
  vendor/monocypher
  vendor/mbedtls/include
  vendor/mbedtls/tf-psa-crypto/include
  ${KEYSTONE_SDK_DIR}/include)

# host
add_executable(${host_bin} ${host_src})
target_link_libraries(${host_bin} ${KEYSTONE_LIB_HOST} ${KEYSTONE_LIB_EDGE})
target_include_directories(${host_bin}
  PUBLIC ${KEYSTONE_SDK_DIR}/include)

# eyrie runtime
set(eyrie_files_to_copy .options_log eyrie-rt loader.bin)
add_eyrie_runtime(${eapp_bin}-eyrie
  ${eyrie_plugins}
  ${eyrie_files_to_copy})

# packaging
add_keystone_package(${eapp_bin}-package
  ${package_name}
  ${package_script}
  ${eyrie_files_to_copy} ${eapp_bin} ${host_bin})

add_dependencies(${eapp_bin}-package ${eapp_bin}-eyrie)
add_dependencies(examples ${eapp_bin}-package)
