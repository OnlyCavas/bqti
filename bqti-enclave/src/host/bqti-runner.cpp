#include <cstring>

#include "edge/edge_call.h"
#include "edge/edge_common.h"
#include "host/Params.hpp"
#include "host/keystone.h"

#include "protocol.h"

using namespace Keystone;

static enclave_req_t g_pending_req;
static enclave_res_t g_received_res;

static void handle_get_request(void *buffer) {
  struct edge_call *ecall = (struct edge_call *)buffer;

  size_t req_offset = sizeof(struct edge_call);
  memcpy((uint8_t *)buffer + req_offset, &g_pending_req, sizeof(g_pending_req));

  size_t edata_offset = req_offset + sizeof(g_pending_req);
  struct edge_data *edata = (struct edge_data *)((uint8_t *)buffer + edata_offset);
  edata->offset = req_offset;
  edata->size   = sizeof(g_pending_req);

  ecall->return_data.call_status     = CALL_STATUS_OK;
  ecall->return_data.call_ret_offset = edata_offset;
  ecall->return_data.call_ret_size   = sizeof(struct edge_data);
}

static void handle_send_result(void *buffer) {
  struct edge_call *ec = (struct edge_call *)buffer;

  memcpy(&g_received_res,
      (uint8_t *)buffer + ec->call_arg_offset,
      sizeof(g_received_res));

  ec->return_data.call_status = CALL_STATUS_OK;
}

int main(int argc, char **argv) {
  Enclave enclave;
  Params params;

  g_pending_req.op           = OP_HASH;
  const char *msg            = "hello from host";

  memcpy(g_pending_req.hash.data, msg, strlen(msg));
  g_pending_req.hash.data_len = strlen(msg);

  params.setFreeMemSize(4 * 1024 * 1024);
  params.setUntrustedSize(256 * 1024);

  enclave.init(argv[1], argv[2], argv[3], params);

  enclave.registerOcallDispatch(incoming_call_dispatch);
  edge_call_init_internals(
      (uintptr_t)enclave.getSharedBuffer(), enclave.getSharedBufferSize());

  edge_call_table[OCALL_GET_REQUEST] = handle_get_request;
  edge_call_table[OCALL_SEND_RESULT] = handle_send_result;

  enclave.run();

  printf("status: %d\n", g_received_res.status);
  for (int i = 0; i < 32; i++) printf("%02x", g_received_res.hash.hash[i]);
  printf("\n");

  return 0;
}
