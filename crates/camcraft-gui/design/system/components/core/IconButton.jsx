import React from 'react';
import { Button } from './Button.jsx';
export function IconButton({label,children,...rest}){return <Button aria-label={label} title={label} icon={children} {...rest} />;}