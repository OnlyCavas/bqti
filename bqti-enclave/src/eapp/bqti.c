#include <stdint.h>
#include <assert.h>
#include <stdio.h>
#include <string.h>
#include <stdbool.h>

#include "app/sealing.h"
#include "app/syscall.h"
#include "monocypher-ed25519.h"

#include "monocypher.h"
#include "psa/crypto.h"
#include "psa/crypto_types.h"

#define HASH_LENGTH 32

static uint8_t g_enclave_pk[32];
static uint8_t g_enclave_sk[64];
static bool    g_initialized = false;

static void enclave_init(void) {
  assert(!g_initialized);

  psa_status_t s = psa_crypto_init();
  assert(s == PSA_SUCCESS);

  struct sealing_key sk;
  get_sealing_key(&sk, sizeof(sk), NULL, 0);
  crypto_ed25519_key_pair(g_enclave_sk, g_enclave_pk, sk.key);

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
    crypto_ed25519_sign(sig, g_enclave_sk, msg, msg_len);
}

const uint8_t *enclave_pubkey(void) {
    assert(g_initialized);
    return g_enclave_pk;
}

int sha256_hash(const uint8_t *input, size_t input_len, uint8_t hash[32]) {
  size_t hash_len;

  psa_status_t s = psa_hash_compute(
      PSA_ALG_SHA_256,
      input, input_len,
      hash, 32, &hash_len);

  return s == PSA_SUCCESS ? 0 : -1;
}

int main(void) {
  enclave_init();

  const char *input = "Test Setup: BQTI within Enclave";
  uint8_t hash[HASH_LENGTH];
  sha256_hash((const uint8_t *)input, strlen(input), hash);

  printf("String: %s\n", input);

  printf("sha256: ");
  for (size_t i = 0; i < HASH_LENGTH; i++) printf("%02x", hash[i]);
  printf("\n");

  const uint8_t *pk = enclave_pubkey();
  printf("pubkey: ");
  for (int i = 0; i < 32; i++) printf("%02x", pk[i]);
  printf("\n");

  enclave_destroy();
  return 0;
}
