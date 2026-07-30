/**
 * Atmospeak Pro gated update Worker.
 *
 * GET /atmospeak/pro/latest.json
 *   Requires X-Atmospeak-License + X-Atmospeak-Activation.
 *   Returns Tauri latest.json whose platform URLs carry short-lived HMAC tokens
 *   (so the stock updater can download installers without custom headers).
 *
 * GET /atmospeak/pro/artifacts/:name?exp=&sig=
 *   Validates HMAC; streams from private R2.
 */

export interface Env {
  PRO_BUCKET: R2Bucket;
  POLAR_ACCESS_TOKEN: string;
  POLAR_ORGANIZATION_ID: string;
  POLAR_LICENSE_BENEFIT_ID?: string;
  PRO_MANIFEST_KEY?: string;
  UPDATE_WINDOW_YEARS?: string;
  /** HMAC secret for time-limited artifact URLs (wrangler secret). */
  ARTIFACT_SIGNING_SECRET: string;
}

interface PolarValidateResponse {
  status?: string;
  benefit_id?: string;
  expires_at?: string | null;
  created_at?: string;
}

interface LatestJson {
  version: string;
  notes?: string;
  pub_date?: string;
  platforms: Record<string, { signature: string; url: string }>;
}

const cors: Record<string, string> = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Headers":
    "Content-Type, X-Atmospeak-License, X-Atmospeak-Activation",
  "Access-Control-Allow-Methods": "GET, OPTIONS",
};

const ARTIFACT_TTL_SECONDS = 60 * 60;

function json(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: { "Content-Type": "application/json", ...cors },
  });
}

function bytesToHex(bytes: ArrayBuffer): string {
  return [...new Uint8Array(bytes)]
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

async function hmacHex(secret: string, message: string): Promise<string> {
  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const sig = await crypto.subtle.sign(
    "HMAC",
    key,
    new TextEncoder().encode(message),
  );
  return bytesToHex(sig);
}

async function signArtifact(
  secret: string,
  name: string,
  exp: number,
): Promise<string> {
  return hmacHex(secret, `${name}:${exp}`);
}

async function assertLicence(
  request: Request,
  env: Env,
): Promise<PolarValidateResponse> {
  const license = request.headers.get("X-Atmospeak-License")?.trim();
  const activation = request.headers.get("X-Atmospeak-Activation")?.trim();
  if (!license || !activation) {
    throw new Response(JSON.stringify({ error: "missing_licence_headers" }), {
      status: 401,
      headers: { "Content-Type": "application/json", ...cors },
    });
  }

  const response = await fetch(
    "https://api.polar.sh/v1/customer-portal/license-keys/validate",
    {
      method: "POST",
      headers: {
        Authorization: `Bearer ${env.POLAR_ACCESS_TOKEN}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({
        key: license,
        organization_id: env.POLAR_ORGANIZATION_ID,
        activation_id: activation,
        conditions: { product: "atmospeak-pro" },
      }),
    },
  );
  if (!response.ok) {
    const text = await response.text();
    throw new Response(
      JSON.stringify({ error: "licence_invalid", detail: text }),
      { status: 403, headers: { "Content-Type": "application/json", ...cors } },
    );
  }

  const polar = (await response.json()) as PolarValidateResponse;
  const status = polar.status ?? "";
  if (status !== "granted" && status !== "active") {
    throw new Response(JSON.stringify({ error: "licence_inactive", status }), {
      status: 403,
      headers: { "Content-Type": "application/json", ...cors },
    });
  }
  if (
    env.POLAR_LICENSE_BENEFIT_ID &&
    polar.benefit_id !== env.POLAR_LICENSE_BENEFIT_ID
  ) {
    throw new Response(JSON.stringify({ error: "wrong_benefit" }), {
      status: 403,
      headers: { "Content-Type": "application/json", ...cors },
    });
  }

  const years = Number(env.UPDATE_WINDOW_YEARS ?? "3");
  const until = (() => {
    if (polar.expires_at) return new Date(polar.expires_at);
    if (!polar.created_at) {
      throw new Response(
        JSON.stringify({
          error: "missing_licence_timestamps",
          detail:
            "Polar response lacked expires_at and created_at; cannot compute update window",
        }),
        {
          status: 403,
          headers: { "Content-Type": "application/json", ...cors },
        },
      );
    }
    const d = new Date(polar.created_at);
    d.setFullYear(d.getFullYear() + years);
    return d;
  })();
  if (Date.now() > until.getTime()) {
    throw new Response(
      JSON.stringify({
        error: "update_window_expired",
        updates_until: until.toISOString(),
      }),
      { status: 403, headers: { "Content-Type": "application/json", ...cors } },
    );
  }

  return polar;
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    if (request.method === "OPTIONS") {
      return new Response(null, { status: 204, headers: cors });
    }
    if (request.method !== "GET") {
      return json({ error: "method_not_allowed" }, 405);
    }

    if (url.pathname === "/atmospeak/pro/latest.json") {
      try {
        await assertLicence(request, env);
      } catch (err) {
        if (err instanceof Response) return err;
        return json({ error: "licence_check_failed", detail: String(err) }, 500);
      }

      const manifestKey = env.PRO_MANIFEST_KEY || "atmospeak/pro/latest.json";
      const object = await env.PRO_BUCKET.get(manifestKey);
      if (!object) return json({ error: "manifest_missing" }, 404);
      const manifest = (await object.json()) as LatestJson;
      const exp = Math.floor(Date.now() / 1000) + ARTIFACT_TTL_SECONDS;
      for (const platform of Object.values(manifest.platforms ?? {})) {
        const artifactName = platform.url.split("/").pop();
        if (!artifactName) continue;
        const sig = await signArtifact(
          env.ARTIFACT_SIGNING_SECRET,
          artifactName,
          exp,
        );
        const artifactUrl = new URL(
          `/atmospeak/pro/artifacts/${encodeURIComponent(artifactName)}`,
          url.origin,
        );
        artifactUrl.searchParams.set("exp", String(exp));
        artifactUrl.searchParams.set("sig", sig);
        platform.url = artifactUrl.toString();
      }
      return new Response(JSON.stringify(manifest), {
        status: 200,
        headers: {
          "Content-Type": "application/json",
          "Cache-Control": "no-store",
          ...cors,
        },
      });
    }

    const artifactMatch = url.pathname.match(
      /^\/atmospeak\/pro\/artifacts\/([^/]+)$/,
    );
    if (artifactMatch) {
      const name = decodeURIComponent(artifactMatch[1]);
      if (name.includes("..") || name.includes("\\") || name.includes("/")) {
        return json({ error: "invalid_artifact" }, 400);
      }
      const exp = Number(url.searchParams.get("exp") || "0");
      const sig = url.searchParams.get("sig") || "";
      if (!exp || !sig || Math.floor(Date.now() / 1000) > exp) {
        return json({ error: "artifact_token_expired" }, 403);
      }
      const expected = await signArtifact(env.ARTIFACT_SIGNING_SECRET, name, exp);
      if (expected !== sig) {
        return json({ error: "artifact_token_invalid" }, 403);
      }
      const key = `atmospeak/pro/artifacts/${name}`;
      const object = await env.PRO_BUCKET.get(key);
      if (!object) return json({ error: "artifact_missing" }, 404);
      const headers = new Headers(cors);
      headers.set(
        "Content-Type",
        object.httpMetadata?.contentType || "application/octet-stream",
      );
      headers.set("Cache-Control", "no-store");
      if (object.httpEtag) headers.set("ETag", object.httpEtag);
      return new Response(object.body, { status: 200, headers });
    }

    return json({ error: "not_found" }, 404);
  },
};
