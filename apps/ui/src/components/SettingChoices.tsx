interface SettingChoicesProps<T extends string | number> {
  id: string;
  label: string;
  value: T;
  options: { value: T; label: string; disabled?: boolean }[];
  onChange: (value: T) => void;
  describedBy?: string;
}

/** Native radios keep short settings lists visible and keyboard accessible. */
export function SettingChoices<T extends string | number>({
  id, label, value, options, onChange, describedBy,
}: SettingChoicesProps<T>) {
  return (
    <fieldset className="setting-choices" role="radiogroup" aria-describedby={describedBy}>
      <legend>{label}</legend>
      <div className="setting-choice-options">
        {options.map((option) => (
          <label key={option.value} className={`setting-choice ${value === option.value ? 'selected' : ''}`}>
            <input type="radio" name={id} value={option.value} checked={value === option.value}
              disabled={option.disabled} onChange={() => onChange(option.value)} />
            <span>{option.label}</span>
          </label>
        ))}
      </div>
    </fieldset>
  );
}
