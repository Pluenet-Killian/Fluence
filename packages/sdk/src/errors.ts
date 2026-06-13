// SPDX-License-Identifier: Apache-2.0

/**
 * Typed transport errors: every non-2xx hub response is an RFC 9457
 * problem document with a stable machine-readable code.
 *
 * @packageDocumentation
 */

import type { Problem } from "./types.js";

/** A hub error response, surfaced as a typed exception. */
export class FluenceProblemError extends Error {
  /** The RFC 9457 problem document returned by the hub. */
  readonly problem: Problem;

  constructor(problem: Problem) {
    super(`${problem.title} (${problem.code}, HTTP ${String(problem.status)})`);
    this.name = "FluenceProblemError";
    this.problem = problem;
  }
}

/**
 * Builds the error for a non-OK response: parses problem+json when
 * possible, falls back to a synthetic `unknown`-coded problem otherwise
 * (a proxy or crash may answer with plain text).
 */
export async function problemFromResponse(response: Response): Promise<FluenceProblemError> {
  try {
    const problem = (await response.json()) as Problem;
    if (typeof problem.code === "string" && typeof problem.status === "number") {
      return new FluenceProblemError(problem);
    }
  } catch {
    // Not JSON — fall through to the synthetic problem.
  }
  return new FluenceProblemError({
    type: "about:blank",
    title: response.statusText === "" ? "Request failed" : response.statusText,
    status: response.status,
    code: "unknown",
  });
}
