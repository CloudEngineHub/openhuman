import { render, screen, fireEvent } from '@testing-library/react';
import { useState } from 'react';
import { describe, it, expect } from 'vitest';
import SettingsSearchBar from '../../src/components/settings/search/SettingsSearchBar';

function Harness() {
  const [v, setV] = useState('hello');
  return <SettingsSearchBar value={v} onValueChange={setV} />;
}

describe('ref forwarding through the shadcn Input', () => {
  it('refocuses the input after clearing', () => {
    render(<Harness />);
    const input = screen.getByTestId('settings-search-input');
    fireEvent.click(screen.getByTestId('settings-search-clear'));
    expect(document.activeElement).toBe(input);
  });
});
