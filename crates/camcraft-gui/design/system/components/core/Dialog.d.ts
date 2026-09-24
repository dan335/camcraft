/** Modal dialog, 4px radius, deep shadow. Renders nothing when closed. */
export interface DialogProps { open: boolean; title: React.ReactNode; children?: React.ReactNode; footer?: React.ReactNode; onClose?: () => void; width?: number; }
export function Dialog(props: DialogProps): JSX.Element | null;