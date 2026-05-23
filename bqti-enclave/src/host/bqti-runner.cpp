#include "enclave_ffi.h"
#include "protocol.h"
#include <cstdint>
#include <cstdio>

void print_hex(const uint8_t *data, size_t data_len) {
  for (int i = 0; i < data_len; i++) printf("%02x", data[i]);
}

void test_pow() {
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

  printf("\n");
  printf("%zu", sizeof(attest_report_t));
  printf("\n");
}

int main(int argc, char **argv) {
  enclave_init(argv[1], argv[2], argv[3]);

  pow_result_t result;
  attest_report_t report;

  uint32_t nonce = 0xDEADBEEF;
  int status = enclave_attest(
    &nonce,
    sizeof(uint32_t),
    reinterpret_cast<uint8_t*>(&report)
  );

  if (status != ENCLAVE_OK) {
    printf("failed to attest");
  } else {
    printf("enclave-hash: ");

    for (size_t i = 0; i < sizeof(report.enclave.hash); i++) {
      printf("%02x", report.enclave.hash[i]);
    }

    printf("\n");
  }

  enclave_destroy();
  return 0;
}
