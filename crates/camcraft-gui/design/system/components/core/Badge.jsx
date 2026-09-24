import React from 'react';
const C={neutral:['var(--surface-3)','var(--text-2)'],ok:['var(--ok-subtle)','var(--ok)'],warn:['var(--warn-subtle)','var(--warn)'],err:['var(--err-subtle)','var(--err)'],info:['var(--info-subtle)','var(--info)'],accent:['var(--accent)','var(--text-on-accent)']};
export function Badge({tone='neutral',dot,mono,children}){const [bg,fg]=C[tone];
  return <span style={{display:'inline-flex',alignItems:'center',gap:6,height:18,padding:'0 6px',background:bg,color:fg,borderRadius:'var(--radius-sm)',fontSize:11,fontWeight:500,fontFamily:mono?'var(--font-mono)':'var(--font-sans)',letterSpacing:mono?0:'.02em',lineHeight:1,whiteSpace:'nowrap'}}>{dot&&<span style={{width:6,height:6,background:fg}}/>}{children}</span>;
}