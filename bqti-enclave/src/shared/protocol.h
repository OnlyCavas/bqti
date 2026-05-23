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
#define OP_HASH 0x01 // Hashing a message
#define OP_SIGN 0x02 // Signing with secret keypair
#define OP_POW 0x03 // Trigger Proof of Work
#define OP_FETCH_PUBKEY 0x04 // FETCH Tee's pub key
#define OP_ATTEST 0x05 // Request a Attestation Report

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

#define DATA_MAX_LENGTH 1024

// Attestation specifics
#define ATTEST_MDSIZE          64
#define ATTEST_DATA_MAXLEN     1024
#define ATTEST_REPORT_SIZE     1352

typedef struct {
  uint8_t hash[ATTEST_MDSIZE];
  uint64_t data_len;
  uint8_t data[ATTEST_DATA_MAXLEN];
  uint8_t signature[SIGNATURE_LENGTH];
} enclave_report_t;

typedef struct {
  uint8_t hash[ATTEST_MDSIZE];
  uint8_t pub_key[PUBKEY_LENGTH];
  uint8_t signature[SIGNATURE_LENGTH];
} sm_report_t;

typedef struct {
  enclave_report_t enclave;
  sm_report_t sm;
  uint8_t dev_pub_key[PUBKEY_LENGTH];
} attest_report_t;

// Enclave Requests
typedef struct { uint32_t challange; uint32_t difficulty; } pow_req_t;
typedef struct { uint8_t data[DATA_MAX_LENGTH]; size_t data_len; } sign_req_t;
typedef struct { uint8_t data[256]; size_t data_len; } hash_req_t;
typedef struct { uint8_t nonce[64]; size_t nonce_len; } attest_req_t;

typedef struct {
  uint32_t op;

  union {
    pow_req_t pow;
    sign_req_t sign;
    hash_req_t hash;
    attest_req_t attest;
  };
} enclave_req_t;

// Enclave Responses
typedef struct {
  uint32_t nonce;
  uint8_t pow[HASH_LENGTH];
  uint8_t pub_key[HASH_LENGTH];
  uint8_t signature[SIGNATURE_LENGTH];
} pow_res_t;

typedef struct {
  uint8_t sig[SIGNATURE_LENGTH];
  uint8_t pb_key[PUBKEY_LENGTH];
} sign_res_t;

typedef struct { uint8_t hash[HASH_LENGTH]; } hash_res_t;
typedef struct { uint8_t pub_key[PUBKEY_LENGTH]; } pubkey_res_t;
typedef struct { attest_report_t report; } attest_res_t;

typedef struct {
  uint32_t op;
  uint32_t status;

  union {
    pow_res_t pow;
    sign_res_t sign;
    hash_res_t hash;
    pubkey_res_t pub_key;
    attest_res_t attest;
  };
} enclave_res_t;

