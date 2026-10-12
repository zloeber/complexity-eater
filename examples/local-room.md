# Open a local room

```sh
printf '# Architecture\n\n## Goals\n\nship\n' > idea.md
ce idea.md --name Ada --port 8000
```

The terminal prints a human URL and a `commonroom://join/` invitation. Open the human URL in a browser on the same machine. The page is the host session.

In another terminal:

```sh
ce mcp --invite 'commonroom://join/...' --name "Agent"
```

Paste a JSON-RPC line such as `{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}`.

The agent can propose. Approve on the human page, then the agent commits with `workspace_commit` and the `proposal_id`. A second join of the same invitation is rejected.
