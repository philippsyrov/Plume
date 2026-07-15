import type { AppearancePreference } from './useAppearance';

const choices: Array<{
  value: AppearancePreference;
  label: string;
  detail: string;
}> = [
  { value: 'system', label: 'System', detail: 'Follow this Mac' },
  { value: 'light', label: 'Light', detail: 'Paper and ink' },
  { value: 'dark', label: 'Dark', detail: 'Low-light workspace' },
];

export function AppearancePanel({
  value,
  onChange,
}: {
  value: AppearancePreference;
  onChange: (next: AppearancePreference) => void;
}) {
  return (
    <fieldset className="plume-appearance-panel">
      <legend>Appearance</legend>
      <p className="plume-appearance-label">Theme</p>
      <div className="plume-appearance-options">
        {choices.map((choice) => (
          <label
            key={choice.value}
            className={`plume-appearance-option${value === choice.value ? ' is-selected' : ''}`}
          >
            <input
              type="radio"
              aria-label={choice.label}
              name="plume-appearance"
              value={choice.value}
              checked={value === choice.value}
              onChange={() => onChange(choice.value)}
            />
            <span>{choice.label}</span>
            <small>{choice.detail}</small>
          </label>
        ))}
      </div>
      <p className="plume-appearance-future">Custom colors are planned for later.</p>
    </fieldset>
  );
}
