/** Square 14px checkbox, amber when checked. */
export interface CheckboxProps { checked?: boolean; defaultChecked?: boolean; onChange?: (v: boolean) => void; label?: React.ReactNode; disabled?: boolean; }
export function Checkbox(props: CheckboxProps): JSX.Element;