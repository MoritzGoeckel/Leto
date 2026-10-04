#!/usr/bin/env node
const readline = require("node:readline");

class Plugin {
  constructor(hooks) {
    this.hooks = hooks;
    this.nextId = 1;
    this.pending = new Map();
  }

  start() {
    const input = readline.createInterface({ input: process.stdin });
    input.on("line", (line) => this.receive(JSON.parse(line)));
  }

  async receive(request) {
    if (request.type === "return") {
      const pending = this.pending.get(request.id);
      if (!pending || pending.name !== request.name) throw new Error("unexpected plugin response");
      this.pending.delete(request.id);
      if (request.error) pending.reject(new Error(request.error.message));
      else pending.resolve(request.value);
      return;
    }
    if (request.type !== "invoke") throw new Error(`unexpected message type: ${request.type}`);
    if (request.name === "init") {
      this.respond(request, { hooks: Object.keys(this.hooks) });
      return;
    }
    const hook = this.hooks[request.name];
    if (!hook) {
      this.respond(request, undefined, new Error(`unknown hook: ${request.name}`));
      return;
    }
    try {
      this.respond(request, await hook(request.params, { callHost: this.callHost.bind(this) }));
    } catch (error) {
      this.respond(request, undefined, error);
    }
  }

  callHost(name, params) {
    const id = this.nextId++;
    process.stdout.write(`${JSON.stringify({ type: "invoke", id, name, params })}\n`);
    return new Promise((resolve, reject) => this.pending.set(id, { name, resolve, reject }));
  }

  respond(request, value, error) {
    const response = { type: "return", id: request.id, name: request.name };
    if (error) response.error = { message: error.message || String(error) };
    else response.value = value === undefined ? null : value;
    process.stdout.write(`${JSON.stringify(response)}\n`);
  }
}

module.exports = { Plugin };
