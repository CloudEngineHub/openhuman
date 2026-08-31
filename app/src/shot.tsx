import { createRoot } from 'react-dom/client';
import './index.css';
import SettingsTabbedPage from './components/settings/layout/SettingsTabbedPage';
import Button from './components/ui/Button';

const Back = () => (
  <Button type="button" variant="tertiary" size="sm" iconOnly aria-label="Back" className="h-8 w-8">
    <svg className="h-5 w-5" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden="true">
      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 19l-7-7 7-7" />
    </svg>
  </Button>
);

const actions = (
  <div className="flex items-center gap-2">
    <div className="rounded-lg border border-line bg-surface px-1 py-1 text-sm text-content-muted">
      <span className="px-2">Copilot</span><span className="px-2">Manual</span>
    </div>
    <Button variant="tertiary" size="sm" iconOnly aria-label="Discard">✕</Button>
    <Button variant="primary" size="sm" iconOnly aria-label="Save">▣</Button>
    <Button variant="primary" size="sm" iconOnly aria-label="Run">▶</Button>
  </div>
);

const title = (
  <input
    type="text" defaultValue="Daily digest to channel" aria-label="Rename"
    className="-mx-1 w-full max-w-lg truncate rounded-md border-0 bg-transparent p-0 px-1 text-inherit [font:inherit] hover:bg-surface-hover focus:bg-surface-hover focus:outline-hidden focus:ring-2 focus:ring-primary-500/30"
  />
);

createRoot(document.getElementById('root')!).render(
  <div className="h-screen p-3">
    <div className="flex h-full gap-3">
      <div className="w-60 shrink-0 rounded-2xl border border-line p-2 text-xs text-content-muted">app sidebar</div>
      <div className="relative min-w-0 flex-1 overflow-hidden rounded-2xl bg-surface">
        <div className="h-full p-4">
          <SettingsTabbedPage title={title} description="Build this automation step by step, then save and run it." leading={<Back />} headerAction={actions} scrollable={false} bodyFullBleed>
            <div className="h-full w-full bg-surface-muted" />
          </SettingsTabbedPage>
        </div>
      </div>
    </div>
  </div>
);
