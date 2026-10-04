#!/usr/bin/env node
const { Plugin } = require("../sdk/javascript/sdk");

new Plugin({
  on_init: async (_params, { callHost }) => {
    await callHost("notify_user", { message: "Example plugin initialized" });
  },
  on_user_message: (message) => ({
    ...message,
    content: typeof message.content === "string"
      ? message.content.toUpperCase()
      : message.content.map((block) => block.type === "text"
        ? { ...block, text: { ...block.text, text: block.text.text.toUpperCase() } }
        : block),
  }),
}).start();
