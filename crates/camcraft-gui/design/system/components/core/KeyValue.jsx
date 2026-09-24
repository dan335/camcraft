import React from 'react';
export function KeyValue({items=[],columns=1}){
  return <dl style={{margin:0,display:'grid',gridTemplateColumns:`repeat(${columns},minmax(0,1fr))`,gap:'6px 24px'}}>
    {items.map(([k,v,u],i)=><div key={i} style={{display:'flex',justifyContent:'space-between',alignItems:'baseline',gap:12,fontSize:13,borderBottom:'1px solid var(--border)',padding:'4px 0'}}><dt style={{color:'var(--text-3)',margin:0}}>{k}</dt><dd style={{margin:0,fontFamily:'var(--font-mono)',fontFeatureSettings:'var(--font-numeric)',color:'var(--text-1)'}}>{v}{u&&<span style={{color:'var(--text-3)',marginLeft:4}}>{u}</span>}</dd></div>)}
  </dl>;
}