/** Fetch the exact display asset produced alongside the native run's report. */
export async function replayAsset(path: string, integrity: string, signal?: AbortSignal) {
  const version = integrity.split(" ", 1)[0];
  const response = await fetch(path + "?v=" + encodeURIComponent(version), { integrity, signal });
  if (!response.ok || !response.body) throw Error("Missing garment replay asset");
  return response;
}

const published: Record<string, string> = JSON.parse(process.env.NEXT_PUBLIC_GEOMETRY_INTEGRITY ?? "{}");

export function publishedGeometry(path: string, signal?: AbortSignal) {
  const integrity = published[path];
  if (!integrity) throw Error("Missing geometry identity for " + path);
  return replayAsset(path, integrity, signal);
}
