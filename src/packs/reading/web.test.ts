import { expect, spyOn, test } from "bun:test";
import { EventEmitter } from "node:events";
import type { ClientRequest, IncomingHttpHeaders } from "node:http";
import * as https from "node:https";
import { Readable } from "node:stream";
import { articleUrl, readHtml } from "./web";

const publicAddress = { address: "93.184.216.34", family: 4 };
const resolve = async () => [publicAddress];
function response(
  chunks: Uint8Array[] = [Buffer.from("<html>fixture</html>")],
  statusCode = 200,
  headers: IncomingHttpHeaders = { "content-type": "text/html; charset=utf-8" },
) {
  return Object.assign(Readable.from(chunks), { statusCode, headers });
}
const neverRequest = async (): Promise<never> => {
  throw new Error("unexpected network request");
};

test.each([
  "http://article.example/",
  "file:///private/data",
  "https://user:password@article.example/",
  "https://article.example:8443/",
])("rejects unsupported URL %s before DNS or request", async (url) => {
  let lookups = 0;
  await expect(
    readHtml(url, {
      resolve: async () => {
        lookups++;
        return [publicAddress];
      },
      request: neverRequest,
    }),
  ).rejects.toThrow("credential-free HTTPS");
  expect(lookups).toBe(0);
});

test.each([
  "127.0.0.1",
  "10.0.0.1",
  "169.254.169.254",
  "100.64.0.1",
  "192.168.0.1",
  "0.0.0.0",
  "192.0.2.1",
  "224.0.0.1",
  "::1",
  "fe80::1",
  "fd00::1",
  "::ffff:127.0.0.1",
  "2001:db8::1",
])(
  "rejects non-public address %s, including mixed DNS answers",
  async (address) => {
    await expect(
      readHtml("https://article.example/", {
        resolve: async () => [
          { address, family: address.includes(":") ? 6 : 4 },
        ],
        request: neverRequest,
      }),
    ).rejects.toThrow("public unicast");
    await expect(
      readHtml("https://article.example/", {
        resolve: async () => [
          publicAddress,
          { address, family: address.includes(":") ? 6 : 4 },
        ],
        request: neverRequest,
      }),
    ).rejects.toThrow("public unicast");
  },
);

test("canonical URL parsing blocks numeric localhost aliases and mapped IPv6 without DNS", async () => {
  for (const url of [
    "https://2130706433/",
    "https://0x7f000001/",
    "https://[::ffff:127.0.0.1]/",
  ])
    await expect(readHtml(url, { request: neverRequest })).rejects.toThrow(
      "public unicast",
    );
  expect(articleUrl("https://article.example/path#fragment").href).toBe(
    "https://article.example/path",
  );
});

test("resolves once and passes the vetted address plus original TLS host to transport", async () => {
  let lookups = 0;
  const body = response([Buffer.from("<p>日本語</p>")]);
  const html = await readHtml("https://article.example/story?q=1", {
    resolve: async (host) => {
      expect(host).toBe("article.example");
      lookups++;
      return [publicAddress];
    },
    request: async (url, address, signal) => {
      expect(url.hostname).toBe("article.example");
      expect(url.pathname + url.search).toBe("/story?q=1");
      expect(address).toEqual(publicAddress);
      expect(signal.aborted).toBe(false);
      return body;
    },
  });
  expect(html).toBe("<p>日本語</p>");
  expect(lookups).toBe(1);
  expect(body.destroyed).toBe(true);
});

test("the default transport connects to the vetted IP while preserving HTTPS identity and Host", async () => {
  let connections = 0;
  const transport = spyOn(https, "get").mockImplementation(
    (...args: unknown[]) => {
      connections++;
      const options = args[0] as https.RequestOptions;
      expect(options.hostname).toBe(publicAddress.address);
      expect(options.port).toBe(443);
      expect(options.servername).toBe("article.example");
      expect(options.path).toBe("/story?q=1");
      expect(options.headers).toEqual({
        Host: "article.example",
        Accept: "text/html, application/xhtml+xml",
        "Accept-Encoding": "identity",
      });
      expect(options.agent).toBe(false);
      expect(options.rejectUnauthorized).not.toBe(false);
      const callback = args[1] as (body: ReturnType<typeof response>) => void;
      queueMicrotask(() => callback(response()));
      return new EventEmitter() as ClientRequest;
    },
  );
  try {
    expect(
      await readHtml("https://article.example/story?q=1", { resolve }),
    ).toBe("<html>fixture</html>");
    expect(connections).toBe(1);
  } finally {
    transport.mockRestore();
  }
});

test("refuses redirects, non-HTML, compression and oversized bodies without following another URL", async () => {
  const cases = [
    response([], 302, { location: "https://127.0.0.1/private" }),
    response([], 500),
    response([], 200, { "content-type": "application/pdf" }),
    response([], 200, {
      "content-type": "text/html",
      "content-encoding": "gzip",
    }),
    response([], 200, {
      "content-type": "text/html",
      "content-length": String(3 * 1024 * 1024),
    }),
    response([Buffer.alloc(1024 * 1024), Buffer.alloc(1024 * 1024 + 1)]),
  ];
  for (const body of cases) {
    let requests = 0;
    await expect(
      readHtml("https://article.example/", {
        resolve,
        request: async () => {
          requests++;
          return body;
        },
      }),
    ).rejects.toThrow();
    expect(requests).toBe(1);
    expect(body.destroyed).toBe(true);
  }
});

test("DNS failure, empty answers and stream errors propagate without an unbounded read", async () => {
  await expect(
    readHtml("https://article.example/", {
      resolve: async () => {
        throw new Error("DNS failed");
      },
      request: neverRequest,
    }),
  ).rejects.toThrow("DNS failed");
  await expect(
    readHtml("https://article.example/", {
      resolve: async () => [],
      request: neverRequest,
    }),
  ).rejects.toThrow("public unicast");
  const body = Object.assign(
    Readable.from(
      (async function* () {
        yield Buffer.from("partial");
        throw new Error("stream failed");
      })(),
    ),
    { statusCode: 200, headers: { "content-type": "text/html" } },
  );
  await expect(
    readHtml("https://article.example/", {
      resolve,
      request: async () => body,
    }),
  ).rejects.toThrow("stream failed");
});

test("cancellation during a body read discards partial HTML and closes the response", async () => {
  const controller = new AbortController();
  const body = Object.assign(
    Readable.from(
      (async function* () {
        yield Buffer.from("<p>partial");
        controller.abort(new Error("cancelled during body"));
        yield Buffer.from("</p>");
      })(),
    ),
    { statusCode: 200, headers: { "content-type": "text/html" } },
  );
  await expect(
    readHtml("https://article.example/", {
      signal: controller.signal,
      resolve,
      request: async () => body,
    }),
  ).rejects.toThrow("cancelled during body");
  expect(body.destroyed).toBe(true);
});

test("a deadline rejects a stalled lookup without making a request", async () => {
  await expect(
    readHtml("https://article.example/", {
      signal: AbortSignal.timeout(1),
      resolve: () => new Promise(() => {}),
      request: neverRequest,
    }),
  ).rejects.toThrow();
});

test("cancellation interrupts pending DNS and transport, and blocks requests after an aborted lookup", async () => {
  for (const phase of ["dns", "transport"]) {
    const controller = new AbortController();
    const pending = readHtml("https://article.example/", {
      signal: controller.signal,
      resolve: phase === "dns" ? () => new Promise(() => {}) : resolve,
      request: () => new Promise(() => {}),
    });
    controller.abort(new Error("cancelled"));
    await expect(pending).rejects.toThrow("cancelled");
  }
  await expect(
    readHtml("https://article.example/", {
      signal: AbortSignal.abort(new Error("already cancelled")),
      resolve,
      request: neverRequest,
    }),
  ).rejects.toThrow("already cancelled");
});
