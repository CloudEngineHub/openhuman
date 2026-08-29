// ---------------------------------------------------------------------------
// SettingsSearchBar
//
// A full-width search field for the settings sidebar. It is purely a
// controlled text input — it does NOT render its own result list. The parent
// (SettingsSidebar) uses the query to filter the visible nav tabs in place.
//
// Built on the shadcn `Input` (`components/assistant-ui/ui/input.tsx`) rather
// than the app's own `TextField`. It used to be a `TextField` overridden into a
// square underline — `rounded-none border-0 border-b border-line focus:ring-0`
// — which is a fourth input shape in an app that already has the shadcn box on
// its composer and dialogs. Standardising on the shared primitive means the
// radius, the border token and the focus ring come from one place and this
// field cannot drift from them again, so the overrides left here are only the
// two that earn their keep: gutters for the icon and the clear button.
// ---------------------------------------------------------------------------
import { useRef } from 'react';

import { useT } from '../../../lib/i18n/I18nContext';
import { Input } from '../../assistant-ui/ui/input';
import Button from '../../ui/Button';
import { CloseIcon } from '../../ui/icons';

interface SettingsSearchBarProps {
  value: string;
  onValueChange: (next: string) => void;
}

const SearchIcon = () => (
  <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden="true">
    <path
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth={2}
      d="M21 21l-4.35-4.35M11 19a8 8 0 100-16 8 8 0 000 16z"
    />
  </svg>
);

const SettingsSearchBar = ({ value, onValueChange }: SettingsSearchBarProps) => {
  const { t } = useT();
  const inputRef = useRef<HTMLInputElement | null>(null);

  return (
    <div data-testid="settings-search" className="relative shrink-0">
      <span className="pointer-events-none absolute inset-y-0 left-2.5 flex items-center text-content-faint">
        <SearchIcon />
      </span>
      <Input
        ref={inputRef}
        type="text"
        aria-label={t('settings.settingsSearch.ariaLabel')}
        autoComplete="off"
        spellCheck={false}
        value={value}
        onChange={event => onValueChange(event.target.value)}
        onKeyDown={event => {
          if (event.key === 'Escape' && value) {
            event.preventDefault();
            onValueChange('');
          }
        }}
        placeholder={t('settings.settingsSearch.placeholder')}
        data-testid="settings-search-input"
        // Only the gutters: `pl-9` clears the leading search glyph and `pr-9`
        // the trailing clear button, replacing the primitive's own `px-2.5` on
        // those sides. Radius, border, height and focus ring are the
        // primitive's and are deliberately not overridden.
        className="pl-9 pr-9"
      />
      {value && (
        <Button
          type="button"
          variant="tertiary"
          size="sm"
          iconOnly
          onClick={() => {
            onValueChange('');
            inputRef.current?.focus();
          }}
          aria-label={t('settings.settingsSearch.clear')}
          data-testid="settings-search-clear"
          className="absolute inset-y-0 right-1 my-auto h-6 w-6 text-content-faint hover:text-content-secondary hover:bg-transparent focus-visible:ring-offset-surface">
          <CloseIcon className="h-4 w-4" />
        </Button>
      )}
    </div>
  );
};

export default SettingsSearchBar;
