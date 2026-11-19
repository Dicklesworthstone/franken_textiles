// Workers Static Assets may answer a video byte-range request with the entire file.
// Keep the asset's validators and serve the requested bytes so browsers can seek.
export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (url.protocol === "http:") {
      url.protocol = "https:";
      return Response.redirect(url.href, 301);
    }

    const asset = await env.ASSETS.fetch(request);
    if (asset.status !== 200 || !url.pathname.startsWith("/media/") ||
        asset.headers.get("Content-Type")?.split(";")[0] !== "video/mp4") return asset;

    const headers = new Headers(asset.headers);
    headers.set("Accept-Ranges", "bytes");
    const range = /^bytes=(\d*)-(\d*)$/i.exec(request.headers.get("Range") ?? "");
    const validator = request.headers.get("If-Range");
    const matchesValidator = !validator || validator === headers.get("ETag") ||
      Date.parse(headers.get("Last-Modified") ?? "") <= Date.parse(validator);
    if (request.method !== "GET" || !range || !(range[1] || range[2]) || !matchesValidator)
      return new Response(asset.body, { headers });

    const file = await asset.blob();
    const start = range[1] ? Number(range[1]) : Math.max(0, file.size - Number(range[2]));
    const end = range[1] && range[2] ? Math.min(Number(range[2]), file.size - 1) : file.size - 1;
    if (start > end || start >= file.size) {
      headers.set("Content-Range", `bytes */${file.size}`);
      headers.set("Content-Length", "0");
      return new Response(null, { status: 416, headers });
    }
    headers.set("Content-Range", `bytes ${start}-${end}/${file.size}`);
    headers.set("Content-Length", String(end - start + 1));
    return new Response(file.slice(start, end + 1), { status: 206, headers });
  },
};
