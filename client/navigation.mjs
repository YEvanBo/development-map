/* EvanBo::DevelopmentMap::V1::046616abf607b642 */
// Framework-independent navigation. Renderers own drawing, this module owns history.
export class MapNavigation {
  constructor(load, {capacity = 100, onChange = () => {}} = {}) {
    this.load = load; this.capacity = Math.max(2, capacity); this.onChange = onChange;
    this.entries = []; this.index = -1; this.views = new Map(); this.serial = 0;
  }
  key(v) { return JSON.stringify([v.project_id, v.revision ?? '', v.node_ref]); }
  remember(state) {
    if (!this.current) return;
    const key = this.key(this.current);
    this.views.delete(key); this.views.set(key, structuredClone(state));
    while (this.views.size > this.capacity) this.views.delete(this.views.keys().next().value);
  }
  async go(target, {replace = false, historyIndex = null} = {}) {
    const serial = ++this.serial;
    this.pending?.abort(); this.pending = new AbortController();
    let data;
    try { data = await this.load(target, this.pending.signal); }
    catch (error) { if (serial !== this.serial || error.name === 'AbortError') return null; throw error; }
    if (serial !== this.serial) return null;
    const current = {project_id: data.project_id, revision: data.revision, node_ref: data.node.node_ref};
    if (historyIndex !== null) this.index = historyIndex;
    else if (replace && this.index >= 0) this.entries[this.index] = current;
    else if (!this.current || this.key(current) !== this.key(this.current)) {
      this.entries.splice(this.index + 1); this.entries.push(current);
      if (this.entries.length > this.capacity) this.entries.shift();
      this.index = this.entries.length - 1;
    }
    this.current = current; this.data = data;
    const saved = structuredClone(this.views.get(this.key(current)) ?? {expanded: [], pages: {}, camera: null});
    // Preserve visual context; invalidate pages if server's branch version changed.
    if (saved.nodeVersion !== data.node.node_version) saved.pages = {};
    saved.nodeVersion = data.node.node_version;
    this.onChange({current, data, view: saved});
    return {current, data, view: saved};
  }
  back() { return this.index > 0 ? this.go(this.entries[this.index - 1], {historyIndex: this.index - 1}) : Promise.resolve(null); }
  forward() { return this.index + 1 < this.entries.length ? this.go(this.entries[this.index + 1], {historyIndex: this.index + 1}) : Promise.resolve(null); }
  up() { return this.data?.parent_ref ? this.go({...this.current, node_ref: this.data.parent_ref}) : Promise.resolve(null); }
  core() { return this.data?.core_architecture_ref ? this.go({...this.current, node_ref: this.data.core_architecture_ref}) : Promise.resolve(null); }
  url() { return `?${new URLSearchParams(this.current ?? {})}`; }
  openUrl(url) {
    const q = new URL(url, 'http://localhost').searchParams;
    if (!q.get('project_id') || !q.get('node_ref')) throw new Error('Missing project or node');
    return this.go({project_id: q.get('project_id'), revision: q.get('revision') ?? undefined, node_ref: q.get('node_ref')}, {replace: true});
  }
}
// Fetches the complete breadcrumb path through bounded ancestor pages in one cancellable navigation.
export function createHttpLoader(fetchJson) {
 return async (view, signal) => {
  const query=new URLSearchParams(Object.entries(view).filter(([,v])=>v!==undefined));
  const focus=await fetchJson(`/v1/map/navigation?${query}`,signal);
  let after=focus.ancestors_next;const seen=new Set();
  while(after){
   if(seen.has(after)) throw new Error('Repeated ancestor cursor');seen.add(after);
   const q=new URLSearchParams({...view,revision:focus.revision,after,limit:'100'});
   const page=await fetchJson(`/v1/map/ancestors?${q}`,signal);
   focus.breadcrumbs=[...page.items].reverse().concat(focus.breadcrumbs);after=page.next;
  }
  focus.ancestors_next=null;return focus;
 };
}
