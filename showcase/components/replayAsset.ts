/** Fetch the exact display asset produced alongside the native run's report. */
export async function replayAsset(path: string, integrity: string, signal: AbortSignal) {
  const response = await fetch(path + "?v=" + encodeURIComponent(integrity), { integrity, signal });
  if (!response.ok || !response.body) throw Error("Missing garment replay asset");
  return response;
}
