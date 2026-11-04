import type { LookupAddress } from "node:dns";
import { lookup } from "node:dns/promises";
import type { IncomingMessage } from "node:http";
import * as https from "node:https";
import { isIP } from "node:net";
import ipaddr from "ipaddr.js";

const MAX_HTML_BYTES = 2 * 1024 * 1024;
type ArticleResponse = Pick<
  IncomingMessage,
  "statusCode" | "headers" | typeof Symbol.asyncIterator
> & { destroy: () => void };

/** Keep the original host for TLS verification while connecting only to a vetted address. */
function request(
  url: URL,
  address: LookupAddress,
  signal: AbortSignal,
): Promise<ArticleResponse> {
  const host = url.hostname.replace(/^\[|\]$/g, "");
  return new Promise((resolve, reject) => {
    https
      .get(
        {
          hostname: address.address,
          family: address.family,
          port: 443,
          path: url.pathname + url.search,
          servername: isIP(host) ? undefined : host,
          agent: false,
          signal,
          headers: {
            Host: url.host,
            Accept: "text/html, application/xhtml+xml",
            "Accept-Encoding": "identity",
          },
        },
        resolve,
      )
      .on("error", reject);
  });
}

async function abortable<T>(
  pending: Promise<T>,
  signal: AbortSignal,
): Promise<T> {
  signal.throwIfAborted();
  let abort!: () => void;
  try {
    return await Promise.race([
      pending,
      new Promise<never>((_resolve, reject) => {
        abort = () => reject(signal.reason);
        signal.addEventListener("abort", abort, { once: true });
      }),
    ]);
  } finally {
    signal.removeEventListener("abort", abort);
  }
}

export function articleUrl(source: string): URL {
  const url = new URL(source);
  if (
    url.protocol !== "https:" ||
    url.username ||
    url.password ||
    (url.port && url.port !== "443")
  ) {
    throw new Error("articles require credential-free HTTPS on port 443");
  }
  url.hash = "";
  return url;
}

/** Network seams are injectable for offline policy tests; no live article is a test prerequisite. */
export async function readHtml(
  source: string,
  options: {
    signal?: AbortSignal;
    resolve?: (host: string) => Promise<LookupAddress[]>;
    request?: typeof request;
  } = {},
): Promise<string> {
  const signal = AbortSignal.any([
    AbortSignal.timeout(20_000),
    ...(options.signal ? [options.signal] : []),
  ]);
  signal.throwIfAborted();
  const url = articleUrl(source);
  const host = url.hostname.replace(/^\[|\]$/g, "");
  const answers = isIP(host)
    ? [{ address: host, family: isIP(host) }]
    : await abortable(
        (options.resolve ?? ((name) => lookup(name, { all: true })))(host),
        signal,
      );
  if (
    !answers.length ||
    answers.some(({ address }) => ipaddr.process(address).range() !== "unicast")
  ) {
    throw new Error(
      "article host must resolve only to public unicast addresses",
    );
  }
  signal.throwIfAborted();
  const response = await abortable(
    (options.request ?? request)(url, answers[0] as LookupAddress, signal),
    signal,
  );
  try {
    if (response.statusCode !== 200)
      throw new Error(
        `article HTTP ${response.statusCode}; redirects are not followed`,
      );
    if (
      !/^(text\/html|application\/xhtml\+xml)(;|$)/i.test(
        response.headers["content-type"] ?? "",
      )
    )
      throw new Error("article response must be HTML");
    if (
      response.headers["content-encoding"] &&
      response.headers["content-encoding"] !== "identity"
    )
      throw new Error("compressed article responses are not supported");
    if (Number(response.headers["content-length"] ?? 0) > MAX_HTML_BYTES)
      throw new Error("article exceeds 2 MiB");
    const chunks: Buffer[] = [];
    let bytes = 0;
    for await (const chunk of response) {
      signal.throwIfAborted();
      const buffer = Buffer.from(chunk);
      bytes += buffer.length;
      if (bytes > MAX_HTML_BYTES) throw new Error("article exceeds 2 MiB");
      chunks.push(buffer);
    }
    return new TextDecoder("utf-8", { fatal: true }).decode(
      Buffer.concat(chunks),
    );
  } finally {
    response.destroy();
  }
}
