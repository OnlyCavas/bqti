#include <stdint.h>
#include <assert.h>
#include <stdio.h>
#include <string.h>
#include <stdbool.h>

#include "verifier/ed25519/ed25519.h"
#include "app/eapp_utils.h"
#include "app/sealing.h"
#include "app/syscall.h"
#include "edge/edge_common.h"

#include "tomcrypt.h"
#include "monocypher.h"

#include "protocol.h"

static uint8_t g_enclave_pk[PUBKEY_LENGTH];
static uint8_t g_enclave_sk[SIGNATURE_LENGTH];
static bool    g_initialized = false;

static enclave_req_t g_request;
static enclave_res_t g_response;

static hash_state g_sha256_state;

int sha256_hash(const uint8_t *input, size_t input_len, uint8_t hash[32]) {
  sha256_init(&g_sha256_state);
  sha256_process(&g_sha256_state, input, input_len);
  sha256_done(&g_sha256_state, hash);

  return 0;
}

static void enclave_init(void) {
  assert(!g_initialized);

  static struct sealing_key sk;
  get_sealing_key(&sk, sizeof(sk), NULL, 0);

  ed25519_create_keypair(g_enclave_pk, g_enclave_sk, sk.key);
  crypto_wipe(&sk, sizeof(sk));

  g_initialized = true;
}

static void enclave_destroy(void) {
    crypto_wipe(g_enclave_sk, sizeof(g_enclave_sk));
    crypto_wipe(g_enclave_pk, sizeof(g_enclave_pk));
    g_initialized = false;
}

void enclave_sign(const uint8_t *msg, size_t msg_len, uint8_t sig[64]) {
    assert(g_initialized);
    ed25519_sign(sig, msg, msg_len, g_enclave_pk, g_enclave_sk);
}

const uint8_t *enclave_pubkey(void) {
    assert(g_initialized);
    return g_enclave_pk;
}

typedef struct {
  uint8_t hash[HASH_LENGTH];
  uint32_t challenge;
  uint32_t nonce;
  uint8_t sig[SIGNATURE_LENGTH];
  uint32_t difficulty;
} pow_ctx_t;

pow_ctx_t pow_init(uint32_t challenge, uint32_t difficulty) {
  return (pow_ctx_t){ .challenge = challenge, .difficulty = difficulty };
}

static bool meets_difficulty(const uint8_t hash[HASH_LENGTH], uint32_t difficulty) {
    uint32_t full_bytes    = difficulty / 8;
    uint32_t remaining_bits = difficulty % 8;

    for (uint32_t i = 0; i < full_bytes; i++) {
        if (hash[i] != 0) return false;
    }

    if (remaining_bits > 0) {
        uint8_t mask = 0xFF << (8 - remaining_bits);
        if ((hash[full_bytes] & mask) != 0) return false;
    }

    return true;
}

void pow_calculate(pow_ctx_t* ctx, const uint8_t pub_key[PUBKEY_LENGTH]) {
  static hash_state pow_hash_state;

  uint8_t hash[HASH_LENGTH];
  uint32_t nonce = 0;

  for (;;) {
    sha256_init(&pow_hash_state);
    sha256_process(&pow_hash_state, pub_key, PUBKEY_LENGTH);
    sha256_process(&pow_hash_state, (const unsigned char *)&ctx->challenge, sizeof(ctx->challenge));
    sha256_process(&pow_hash_state, (const unsigned char *)&nonce, sizeof(nonce));
    sha256_done(&pow_hash_state, hash);

    if (meets_difficulty(hash, ctx->difficulty)) {
      break;
    }

    nonce++;
  }

  ctx->nonce = nonce;
  memcpy(ctx->hash, hash, HASH_LENGTH);
}

void pow_sig(pow_ctx_t* pow) {
  uint8_t signature[SIGNATURE_LENGTH];
  enclave_sign(pow->hash, HASH_LENGTH, signature);
  memcpy(pow->sig, signature, SIGNATURE_LENGTH);
}

int main(void) {
  struct edge_data retdata;
  ocall(OCALL_GET_REQUEST, NULL, 0, &retdata, sizeof(retdata));
  copy_from_shared(&g_request, retdata.offset, retdata.size);

  enclave_init();

  g_response.op     = g_request.op;
  g_response.status = ENCLAVE_OK;

  switch (g_request.op) {
    case OP_HASH: {

      if (sha256_hash(g_request.hash.data, g_request.hash.data_len,
            g_response.hash.hash) != 0)
        g_response.status = ENCLAVE_ERR_HASH;

      break;
    }

    case OP_SIGN: {
      enclave_sign(
        g_request.sign.data,
        g_request.sign.data_len,
        g_response.sign.sig
      );

      memcpy(g_response.sign.pb_key, enclave_pubkey(), PUBKEY_LENGTH);
      break;
    }

    case OP_POW: {
      pow_ctx_t pow_ctx = pow_init(g_request.pow.challange, g_request.pow.difficulty);

      pow_calculate(&pow_ctx, enclave_pubkey());
      pow_sig(&pow_ctx);

      g_response.pow.nonce = pow_ctx.nonce;
      memcpy(g_response.pow.pow, pow_ctx.hash, HASH_LENGTH);
      memcpy(g_response.pow.signature, pow_ctx.sig, SIGNATURE_LENGTH);
      memcpy(g_response.pow.pub_key, enclave_pubkey(), HASH_LENGTH);

      break;
    }

    case OP_FETCH_PUBKEY: {
      memcpy(g_response.pub_key.pub_key, enclave_pubkey(), PUBKEY_LENGTH);
      break;
    }

    default:
      g_response.status = ENCLAVE_ERR_INVALID_OP;
  }

  ocall(OCALL_SEND_RESULT, &g_response, sizeof(g_response), NULL, 0);

  enclave_destroy();
  EAPP_RETURN(0);
}
