import { exhibits, project, repo, workbenches } from "./links";
import "./site.css";

const year = 2026;

export default function SiteFooter() {
  return <footer className="ft-foot">
    <div className="wrap">
      <div className="ft-foot-lead">
        <p className="ft-foot-line">Check it flat. Sew it in simulation. <em>Then make a muslin.</em></p>
        <p className="ft-foot-note">FrankenTextiles checks that a pattern is consistent and predicts how it hangs on a fitted body.
          Whether a garment fits a person is settled by a sewn sample.</p>
      </div>
      <nav className="ft-foot-nav" aria-label="Site map">
        <div><h2 className="mono">The exhibition</h2>
          <ul>{exhibits.map(item => <li key={item.href}><a href={item.href}>{item.label}</a></li>)}</ul></div>
        <div><h2 className="mono">Workbenches</h2>
          <ul>{workbenches.map(item => <li key={item.href}><a href={item.href}>{item.label}</a></li>)}
            <li><a href="/workbench/calibration/calib-sheet-a4.pdf">Calibration sheet · A4</a></li>
            <li><a href="/workbench/calibration/calib-sheet-letter.pdf">Calibration sheet · Letter</a></li></ul></div>
        <div><h2 className="mono">The project</h2>
          <ul>{project.map(item => <li key={item.href}><a href={item.href}>{item.label}</a></li>)}
            <li><a href={repo}>Source on GitHub</a></li>
            <li><a href={`${repo}/blob/main/FEATURE_PARITY.md`}>Parity status</a></li>
            <li><a href={`${repo}/blob/main/LICENSE`}>Licence</a></li></ul></div>
        <div className="ft-foot-privacy"><h2 className="mono">On your device</h2>
          <p>The workbenches run in your browser. Patterns, measurements and photographs you open stay on your device. Nothing is uploaded.</p>
          <p>Every body on this site is synthetic. No real person is measured or shown.</p></div>
      </nav>
      <div className="ft-foot-base">
        <a className="wordmark" href="/" aria-label="FrankenTextiles home"><span className="mark" aria-hidden="true"><i/><i/><i/><i/></span>Franken<span>Textiles</span></a>
        <p>© {year} Jeffrey Emanuel · MIT licence with the OpenAI/Anthropic rider</p>
      </div>
    </div>
  </footer>;
}
