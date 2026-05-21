#pragma once
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

  typedef struct {
    uint8_t  pow[32];
    uint32_t nonce;
    uint8_t  sig[64];
    uint8_t  pub_key[32];
  } pow_result_t;

  int  enclave_init(const char *eapp_path, const char *runtime_path, const char *loader_path);

  int  enclave_run_pow(uint32_t challenge, uint32_t difficulty, pow_result_t *out);

  void enclave_destroy(void);

#ifdef __cplusplus
}
#endif
