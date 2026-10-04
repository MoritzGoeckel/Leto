#!/usr/bin/env node
const readline = require("node:readline");

const input = readline.createInterface({ input: process.stdin });

input.on("line", (line) => {
  const request = JSON.parse(line);
  if (request.type === "invoke" && request.name === "init") {
    process.stdout.write(`${JSON.stringify({ type: "return", id: request.id, name: request.name, value: { hooks: ["on_init"] } })}\n`);
  } else if (request.type === "invoke" && request.name === "on_user_message") {
    process.stdout.write(`${JSON.stringify({ type: "return", id: request.id, name: request.name, value: request.params })}\n`);
  } else if (request.type === "invoke") {
    process.stdout.write(`${JSON.stringify({ type: "return", id: request.id, name: request.name, value: { message: "Example plugin initialized", params: request.params } })}\n`);
  } else {
    process.stdout.write(`${JSON.stringify({ type: "return", id: request.id, name: request.name, value: null })}\n`);
    return;
  }
});
