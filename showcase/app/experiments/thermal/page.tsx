import type { Metadata } from "next";
import ThermalSandwich from "../../../components/ThermalSandwich";

export const metadata: Metadata = {
  title: "Thermal sandwich · FrankenTextiles",
  description:
    "Stack measured neoprene and bovine leather sheets from the FrankenTextiles catalog, declare explicit interface resistance, and compute steady 1D heat flux and surface temperatures in local Rust WebAssembly.",
  alternates: { canonical: "/experiments/thermal/" },
};

export default function ThermalPage() {
  return <ThermalSandwich />;
}
