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

int main(void) {
  struct edge_data retdata;
  ocall(OCALL_GET_REQUEST, NULL, 0, &retdata, sizeof(retdata));
  copy_from_shared(&g_request, retdata.offset, retdata.size);

  g_response.op     = g_request.op;
  g_response.status = ENCLAVE_OK;

  switch (g_request.op) {
    case OP_HASH:

      if (sha256_hash(g_request.hash.data, g_request.hash.data_len,
            g_response.hash.hash) != 0)
        g_response.status = ENCLAVE_ERR_HASH;

      break;

    case OP_SIGN:
      enclave_init();

      enclave_sign(g_request.sign.message, g_request.sign.message_len,
          g_response.sign.sig);

      memcpy(g_response.sign.pb_key, enclave_pubkey(), PUBKEY_LENGTH);

      enclave_destroy();
      break;

    default:
      g_response.status = ENCLAVE_ERR_INVALID_OP;
  }

  ocall(OCALL_SEND_RESULT, &g_response, sizeof(g_response), NULL, 0);

  EAPP_RETURN(0);
}
