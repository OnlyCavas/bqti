#include "enclave_ffi.h"
#include "edge/edge_call.h"
#include "edge/edge_common.h"
#include "host/Enclave.hpp"
#include "host/Params.hpp"
#include "protocol.h"

using namespace Keystone;

static Enclave g_enclave;
static enclave_req_t g_enclave_request;
static enclave_res_t g_enclave_response;
static bool g_initialized = false;

static void handle_get_request(void *buffer) {
  struct edge_call *ecall = (struct edge_call *)buffer;

  size_t req_offset = sizeof(struct edge_call);
  memcpy((uint8_t *)buffer + req_offset, &g_enclave_request,
         sizeof(g_enclave_request));

  size_t edata_offset = req_offset + sizeof(g_enclave_request);
  struct edge_data *edata =
      (struct edge_data *)((uint8_t *)buffer + edata_offset);
  edata->offset = req_offset;
  edata->size = sizeof(g_enclave_request);

  ecall->return_data.call_status = CALL_STATUS_OK;
  ecall->return_data.call_ret_offset = edata_offset;
  ecall->return_data.call_ret_size = sizeof(struct edge_data);
}

static void handle_send_result(void *buffer) {
  struct edge_call *ec = (struct edge_call *)buffer;

  memcpy(&g_enclave_response, (uint8_t *)buffer + ec->call_arg_offset,
         sizeof(g_enclave_response));

  ec->return_data.call_status = CALL_STATUS_OK;
}

int enclave_init(const char *eapp_path, const char *runtime_path,
                 const char *loader_path) {
  Params params;
  params.setFreeMemSize(4 * 1024 * 1024);
  params.setUntrustedSize(256 * 1024);

  g_enclave.init(eapp_path, runtime_path, loader_path, params);
  g_enclave.registerOcallDispatch(incoming_call_dispatch);

  edge_call_init_internals((uintptr_t)g_enclave.getSharedBuffer(),
                           g_enclave.getSharedBufferSize());

  edge_call_table[OCALL_GET_REQUEST] = handle_get_request;
  edge_call_table[OCALL_SEND_RESULT] = handle_send_result;

  g_initialized = true;
  return 0;
}

int enclave_run_pow(uint32_t challenge, uint32_t difficulty,
                    pow_result_t *out) {
  if (!g_initialized)
    return -1;

  g_enclave_request.op = OP_POW;
  g_enclave_request.pow.challange = challenge;
  g_enclave_request.pow.difficulty = difficulty;

  g_enclave.run();

  memcpy(out->pow, g_enclave_response.pow.pow, HASH_LENGTH);
  memcpy(out->sig, g_enclave_response.pow.signature, SIGNATURE_LENGTH);
  memcpy(out->pub_key, g_enclave_response.pow.pub_key, HASH_LENGTH);
  out->nonce = g_enclave_response.pow.nonce;

  return g_enclave_response.status;
}

void enclave_destroy(void) { g_initialized = false; }
