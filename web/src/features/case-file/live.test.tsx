import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { card } from "../../fixtures";
import { caseFile } from "./fixtures";
import { useCaseFile, useOpenRequest } from "./live";

function wrapper() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

describe("useCaseFile", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("refetches when the stream changes the request's card, and only then", async () => {
    const fetch = vi.fn(async () => new Response(JSON.stringify(caseFile()), { status: 200 }));
    vi.stubGlobal("fetch", fetch);
    const first = card({ id: 1, phase: "Making the change" });
    const { result, rerender } = renderHook(({ shown }) => useCaseFile(1, shown), {
      wrapper: wrapper(),
      initialProps: { shown: first },
    });
    await waitFor(() => expect(result.current.file?.revision).toBe(40));
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(fetch).toHaveBeenCalledWith("/api/requests/1", expect.anything());
    rerender({ shown: first });
    expect(fetch).toHaveBeenCalledTimes(1);
    rerender({ shown: card({ id: 1, phase: "Running checks" }) });
    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(2));
    expect(result.current.file?.revision).toBe(40);
  });

  it("reports a request the console doesn't know", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("No request has that id.\n", { status: 404 })));
    const { result } = renderHook(() => useCaseFile(99, undefined), { wrapper: wrapper() });
    await waitFor(() => expect(result.current.error).toBe("This request isn't in the store."));
  });
});

describe("useOpenRequest", () => {
  afterEach(() => window.history.replaceState(null, "", "/"));

  it("keeps the open request in the address, so back closes it", () => {
    window.history.replaceState(null, "", "/?request=3");
    const { result } = renderHook(() => useOpenRequest());
    expect(result.current.id).toBe(3);
    act(() => result.current.open(5));
    expect(window.location.search).toBe("?request=5");
    expect(result.current.id).toBe(5);
    act(() => result.current.close());
    expect(window.location.search).toBe("");
    expect(result.current.id).toBeNull();
    act(() => {
      window.history.pushState(null, "", "/?request=8");
      window.dispatchEvent(new PopStateEvent("popstate"));
    });
    expect(result.current.id).toBe(8);
  });
});
