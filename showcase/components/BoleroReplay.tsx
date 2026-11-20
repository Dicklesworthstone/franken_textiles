import CodeBlock from "./site/CodeBlock";
import runs from "../public/exhibits/poses/runs.json";

export default function BoleroReplay({ chest, angle, repository = false }: {
  chest: 48 | 52 | 54; angle: 35 | 75 | 90; repository?: boolean;
}) {
  const chestMm = chest * 10;
  const key = `${chestMm}-${angle}` as keyof typeof runs.cases;
  const source = "/exhibits/poses/";
  const prefix = repository ? `showcase/public${source}` : "";
  const output = `bolero-${key}`;
  const lines = [
    `ftx drape ${prefix}bolero-start.json`,
    `  --measurements ${prefix}chest-${chestMm}.json --fit ${prefix}fit-spec.json`,
    `  --arm-deg ${angle} --spacing-mm 12 --self-contact-mm 2 --self-mu 0.4`,
    `  --drape 96 --max-settle-steps 256 --out ${output}`,
  ];
  const command = lines.join(" \\\n") + `\nftx viewer ${output} --check-contact`;
  return <>
    <p><a href={source + "bolero-start.json"} download>Cut and starting placements</a>{" · "}
      <a href={`${source}chest-${chestMm}.json`} download>Synthetic tapes · {chestMm} mm chest</a>{" · "}
      <a href={source + "fit-spec.json"} download>Fit specification</a></p>
    <p>{repository ? "Run from the repository root" : "Save the three files together and run from that directory"} with
      {" "}<code>ftx</code> and the converted Anny 0.6.1 model. The second command checks the recorded cloth and body for crossings.</p>
    <CodeBlock title={`Bolero · ${chest} cm chest · ${angle}° arms`} code={command}/>
    <p>Reference SHA-256 for <code>{output}/fold.pc2</code>{" · "}
      <code style={{ overflowWrap: "anywhere" }}>{runs.cases[key].sourcePc2Sha256}</code></p>
  </>;
}
