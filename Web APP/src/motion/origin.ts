export interface MotionOrigin { x:number; y:number; width:number; height:number }
let last:{element:HTMLElement;rect:MotionOrigin;time:number}|undefined;
export function rememberOrigin(element:HTMLElement) {
  const r=element.getBoundingClientRect();last={element,rect:{x:r.x,y:r.y,width:r.width,height:r.height},time:performance.now()};return last.rect;
}
export function originElement():HTMLElement|null { return last&&performance.now()-last.time<1500&&last.element.isConnected?last.element:null; }
export function currentOrigin():MotionOrigin|null {
  if(last&&performance.now()-last.time<1500)return last.element.isConnected?rememberOrigin(last.element):last.rect;
  const active=document.activeElement;
  return active instanceof HTMLElement&&active.matches('button,[role="button"],[role="menuitem"],[role="option"]')?rememberOrigin(active):null;
}
export function installMotionOrigins() {
  const capture=(event:Event)=>{const element=event.target instanceof Element?event.target.closest<HTMLElement>('button,[role="button"],[role="menuitem"],[role="option"]'):null;if(element)rememberOrigin(element);};
  document.addEventListener('pointerdown',capture,true);
  // Exit animations keep Radix content mounted, but not interactive.
  const surfaces='[data-slot="select-content"]';
  const exits=new MutationObserver(records=>{for(const record of records){const node=record.target;if(node instanceof HTMLElement&&node.matches(surfaces))node.inert=node.dataset.state==='closed';}});
  exits.observe(document.body,{subtree:true,attributes:true,attributeFilter:['data-state']});
  document.addEventListener('keydown',event=>{if(event.key==='Enter'||event.key===' ')capture(event);},true);
}
