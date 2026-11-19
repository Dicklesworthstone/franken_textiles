import TrouserStudy from "../../../components/trouser-study/TrouserStudy";
import study from "../../../public/exhibits/trouser-study/study.json";
import "../../../components/trouser-study/study.css";

export const metadata = {
  title:"Room to move · FrankenTextiles",
  description:"Bend a fitted form, change the elastic, and slice through gathered trousers to see the space inside a sewn pattern.",
  alternates:{canonical:"/studies/trousers/"},
};
export default function Page(){return <TrouserStudy study={study}/>;}
