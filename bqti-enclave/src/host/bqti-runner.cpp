#include "enclave_ffi.h"
#include "protocol.h"
#include <cstdio>

void print_hex(const uint8_t *data, size_t data_len) {
  for (int i = 0; i < data_len; i++) printf("%02x", data[i]);
}

int main(int argc, char **argv) {
  enclave_init(argv[1], argv[2], argv[3]);

  pow_result_t result;
  int status = enclave_run_pow(0xDEADBEEF, 20, &result);

  printf("Performing a Proof of Work Calculation\n");

  printf("\n");

  printf("PoW value: ");
  print_hex(result.pow, HASH_LENGTH);
  printf("\n");

  printf("\n");
  printf("---- Values (pub_key | challenge | nonce) ----\n");
  printf("\tPublic Key: ");
  print_hex(result.pub_key, HASH_LENGTH);
  printf("\n");

  printf("\n");
  printf("\tChallenge: %u\n", 0xDEADBEEF);
  printf("\tNonce: %u\n", result.nonce);

  printf("\n");
  printf("---- Signature ----\n");
  print_hex(result.sig, SIGNATURE_LENGTH);
  printf("\n-------------------\n");

  printf("\n");
  printf("status: %d\n", status);

  enclave_destroy();
  return 0;
}
