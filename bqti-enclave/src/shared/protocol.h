#pragma once
#include <stdint.h>
#include <stddef.h>

#define HASH_LENGTH 32
#define PUBKEY_LENGTH 32
#define SIGNATURE_LENGTH 64

// KEYSTONE OCALL
#define OCALL_GET_REQUEST 1
#define OCALL_SEND_RESULT 2

// CAPABILITIES
#define OP_HASH 0x01
#define OP_SIGN 0x02

// CAPABILITIES RESPONSE VALUES
#define ENCLAVE_OK              0
#define ENCLAVE_ERR_GENERIC    -1
#define ENCLAVE_ERR_INVALID_OP -2
#define ENCLAVE_ERR_CRYPTO     -3
#define ENCLAVE_ERR_HASH       -4
#define ENCLAVE_ERR_SIGN       -5
#define ENCLAVE_ERR_POW        -6
#define ENCLAVE_ERR_UNINIT     -7
#define ENCLAVE_ERR_OVERFLOW   -8

// OCAL specifics
#define OCALL_BQTI_RESULT 1

typedef struct { uint8_t challange[32]; uint8_t difficulty; } pow_req_t;
typedef struct { uint8_t message[32]; size_t message_len; } sign_req_t;
typedef struct { uint8_t data[256]; size_t data_len; } hash_req_t;

typedef struct {
  uint32_t op;

  union {
    pow_req_t pow;
    sign_req_t sign;
    hash_req_t hash;
  };
} enclave_req_t;

typedef struct { uint64_t nonce; } pow_res_t;

typedef struct {
  uint8_t sig[SIGNATURE_LENGTH];
  uint8_t pb_key[PUBKEY_LENGTH];
} sign_res_t;

typedef struct { uint8_t hash[HASH_LENGTH]; } hash_res_t;

typedef struct {
  uint32_t op;
  uint32_t status;

  union {
    pow_res_t pow;
    sign_res_t sign;
    hash_res_t hash;
  };
} enclave_res_t;

