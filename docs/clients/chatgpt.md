# ChatGPT

ChatGPT connects to remote MCP servers; it does not directly launch this
repository’s local stdio process. Therefore this path requires a deployed
HTTPS endpoint and a separate MCP bearer token or an authentication system
provided by the deployment.

## Current setup

1. Run the MCP in HTTP mode behind TLS or a secure tunnel.
2. Confirm the endpoint is reachable at its public `/mcp` URL.
3. In ChatGPT web, enable Developer Mode if your plan and workspace permit it.
4. Go to **Settings → Apps → Create** or the custom-app flow.
5. Enter the MCP endpoint and authentication metadata.
6. Scan the tools, review permissions, and test read-only calls first.

Do not expose the UniFi API key to ChatGPT. The remote MCP process should hold
that key in a secret file or server-side secret store and call the UniFi
connector itself.

ChatGPT availability and write support depend on the account and workspace
plan. OpenAI currently documents full MCP app support and write actions as
rolling out for Business, Enterprise, and Edu, while local MCP processes are
not directly connectable. See the [official developer mode and MCP apps guide](https://help.openai.com/en/articles/12584461).
