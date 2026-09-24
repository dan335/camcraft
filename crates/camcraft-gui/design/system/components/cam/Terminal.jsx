import React from 'react';
export const termColors={pass:'var(--ok)',FAIL:'var(--err)',dim:'var(--text-3)',bold:'var(--text-1)',accent:'var(--accent)'};
export function Terminal({title,lines=[],children,height,style}){
  return <div style={{background:'var(--gray-0)',border:'1px solid var(--border)',borderRadius:'var(--radius-sm)',fontFamily:'var(--font-mono)',fontSize:12.5,lineHeight:1.6,color:'var(--text-2)',overflow:'hidden',...style}}>
    {title&&<div style={{height:28,display:'flex',alignItems:'center',padding:'0 12px',borderBottom:'1px solid var(--border)',fontSize:11,color:'var(--text-3)'}}>{title}</div>}
    <pre style={{margin:0,padding:12,height,overflow:'auto',whiteSpace:'pre-wrap'}}>{lines.map((l,i)=>{const seg=Array.isArray(l)?l:[[l]];return <div key={i}>{seg.map(([t,c],j)=><span key={j} style={{color:c?termColors[c]||c:undefined,fontWeight:c==='bold'?600:400}}>{t}</span>)}</div>})}{children}</pre>
  </div>;
}