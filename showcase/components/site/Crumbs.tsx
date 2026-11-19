import "./site.css";

// The trail from the exhibition to the current page. The last entry is the page itself.
export default function Crumbs({ trail }: { trail: readonly (readonly [label: string, href?: string])[] }) {
  return <nav className="wrap ft-crumbs" aria-label="Breadcrumb"><ol>
    {trail.map(([label, href], index) => <li key={label}>
      {href ? <a href={href}>{label}</a> : <span aria-current={index === trail.length - 1 ? "page" : undefined}>{label}</span>}
    </li>)}
  </ol></nav>;
}
