import React from 'react';
const C={error:'var(--err)',warn:'var(--warn)',info:'var(--info)',ok:'var(--ok)'};
export function Diagnostic({level='info',code,message,location,children}){
  return <div style={{display:'grid',gridTemplateColumns:'56px 1fr',gap:'0 12px',padding:'8px 12px',borderLeft:'2px solid '+C[level],background:'var(--surface-2)',border:'1px solid var(--border)',borderLeftWidth:2,borderLeftColor:C[level],borderRadius:'var(--radius-sm)',fontSize:13}}>
    <span style={{fontFamily:'var(--font-mono)',fontSize:11,color:C[level],textTransform:level==='error'?'uppercase':'none',paddingTop:2}}>{level}</span>
    <div><div style={{color:'var(--text-1)'}}>{code&&<code style={{color:'var(--text-3)',marginRight:8,fontSize:12}}>{code}</code>}{message}</div>
    {location&&<div style={{fontFamily:'var(--font-mono)',fontSize:12,color:'var(--text-3)',marginTop:2}}>{location}</div>}{children&&<div style={{marginTop:6,color:'var(--text-2)'}}>{children}</div>}</div>
  </div>;
}