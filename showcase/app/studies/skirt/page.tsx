import SiteHeader from "../../../components/site/SiteHeader";
import SiteFooter from "../../../components/site/SiteFooter";
import Atelier from "../../../components/skirt-atelier/Atelier";

export const metadata={
  title:"The skirt atelier · FrankenTextiles",
  description:"Explore six sewn panels on fitted adult forms. Compare two hems, replay the native cloth simulation and print the full-size sewing patterns.",
  alternates:{canonical:"/studies/skirt/"},
};
export default function Page(){return <><SiteHeader/><main id="main"><Atelier/></main><SiteFooter/></>;}
