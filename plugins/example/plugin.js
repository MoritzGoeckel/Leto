#!/usr/bin/env node
const readline = require("node:readline");

const input = readline.createInterface({ input: process.stdin });

input.on("line", (line) => {
  const request = JSON.parse(line);
  let result;

  if (request.method === "init") {
    result = { hooks: ["on_init"] };
  } else if (request.method === "on_init") {
    result = { message: "Example plugin initialized", params: request.params };
  } else {
    process.stdout.write(`${JSON.stringify({ jsonrpc: "2.0", id: request.id, error: { code: -32601, message: "Method not found" } })}\n`);
    return;
  }

  process.stdout.write(`${JSON.stringify({ jsonrpc: "2.0", id: request.id, result })}\n`);
});
