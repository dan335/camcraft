/** Rectangular toggle (no pill). */
export interface SwitchProps { checked?: boolean; defaultChecked?: boolean; onChange?: (v: boolean) => void; label?: React.ReactNode; disabled?: boolean; }
export function Switch(props: SwitchProps): JSX.Element;