import { HandoffCoordinator, type Env } from "./coordinator";

export { HandoffCoordinator };

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    if (url.pathname.startsWith("/api/")) {
      const stub = env.COORDINATOR.get(env.COORDINATOR.idFromName("personal-handoff"));
      return stub.fetch(request);
    }
    if (request.method !== "GET" && request.method !== "HEAD") {
      return new Response("Not found", { status: 404 });
    }
    return env.ASSETS.fetch(request);
  },
};
