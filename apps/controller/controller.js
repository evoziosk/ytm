const $ = (id) => document.getElementById(id);
function random(bytes) {
  return Array.from(crypto.getRandomValues(new Uint8Array(bytes)), (b) =>
    b.toString(16).padStart(2, '0'),
  ).join('');
}
const params = new URLSearchParams(
  location.hash.slice(1) || sessionStorage.getItem('ytm.invitation') || '',
);
if (location.hash) {
  sessionStorage.setItem('ytm.invitation', location.hash.slice(1));
  history.replaceState(null, '', location.pathname);
}
let session = params.get('session');
let mode = params.get('mode') === 'personal' ? 'personal' : 'group';
let credential;
let socket,
  snapshot,
  participant,
  stopped = true,
  reconnectTimer,
  lostTimer,
  expired = false;
function hostReturned() {
  clearTimeout(lostTimer);
  lostTimer = undefined;
}
function hostLost() {
  if (lostTimer || stopped) return;
  lostTimer = setTimeout(() => {
    stopped = true;
    expired = true;
    clearTimeout(reconnectTimer);
    socket?.close();
    discard();
    fail(
      'Session expired after ten minutes without the host. Request a new invitation.',
    );
  }, 600000);
}
let pending = new Map();
let queue = [],
  queueRevision = -1,
  queueTotal = 0,
  queueLoading = false;
let results = [],
  resultTotal = 0,
  searchQuery = '';
let collections = [],
  browseId = '';
$('host').textContent = `Host: ${location.host}`;
if (session) {
  $('manual').hidden = true;
  $('status').textContent =
    `${mode === 'personal' ? 'Personal remote' : 'Group control'} · request host approval to connect.`;
}
function fail(message) {
  $('error').textContent = message;
}
function request(command) {
  if (!socket || socket.readyState !== WebSocket.OPEN || !participant?.approved)
    return Promise.reject(
      new Error('Waiting for host admission or connection.'),
    );
  const id = random(16);
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(
        new Error(
          'No acknowledgement. State will refresh; this command will not be replayed.',
        ),
      );
    }, 30000);
    pending.set(id, { resolve, reject, timer });
    socket.send(JSON.stringify({ type: 'request', request: { id, command } }));
  });
}
function discard() {
  for (const entry of pending.values()) {
    clearTimeout(entry.timer);
    entry.reject(
      new Error('Connection changed. Uncertain commands are never replayed.'),
    );
  }
  pending.clear();
  participant = undefined;
  $('controls').hidden = true;
}
async function connect() {
  if (!session) {
    if (location.pathname.startsWith('/controller/')) {
      fail('Open the full invitation link from your host.');
      return;
    }
    try {
      const response = await fetch('/session', {
        credentials: 'omit',
        cache: 'no-store',
        signal: AbortSignal.timeout(5000),
      });
      if (!response.ok)
        throw new Error(
          'Enable sharing on the host and update its app, or use the full invitation link.',
        );
      const metadata = await response.json();
      if (
        metadata.version !== 1 ||
        !/^[a-f0-9]{64}$/.test(metadata.session_id) ||
        !['personal', 'group'].includes(metadata.mode)
      )
        throw new Error('This address is not a compatible YTM Listen host.');
      session = metadata.session_id;
      mode = metadata.mode;
      $('host').textContent = `Host: ${metadata.host_name}`;
      $('manual').hidden = true;
    } catch (e) {
      fail(e.message || 'Could not reach the host.');
      return;
    }
  }
  const storage = mode === 'personal' ? localStorage : sessionStorage;
  const storageKey =
    mode === 'personal' ? 'ytm.device' : `ytm.group.${session}`;
  credential = storage.getItem(storageKey);
  if (!credential) {
    credential = random(32);
    storage.setItem(storageKey, credential);
  }
  stopped = false;
  clearTimeout(reconnectTimer);
  fail('');
  $('connect').hidden = true;
  $('disconnect').hidden = false;
  $('status').textContent = 'Connecting to host…';
  const relay = location.pathname.startsWith('/controller/');
  const url = new URL(relay ? `/control/${session}` : '/ws', location.origin);
  url.protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
  socket = new WebSocket(url);
  socket.onopen = () =>
    socket.send(
      JSON.stringify({
        type: 'hello',
        version: 1,
        session_id: session,
        invitation: params.get('invite') || '',
        credential,
        name: $('name').value.trim() || 'Browser',
        mode,
      }),
    );
  socket.onmessage = ({ data }) => {
    let message;
    try {
      message = JSON.parse(data);
    } catch {
      return;
    }
    if (message.type === 'admission') {
      hostReturned();
      participant = message.participant;
      $('status').textContent = participant.approved
        ? 'Connected to host'
        : 'Waiting for the host to approve this device…';
    } else if (message.type === 'state') {
      hostReturned();
      snapshot = message.snapshot;
      // A saved credential's public ID is learned in admission before state.
      if (participant)
        participant =
          snapshot.participants.find((p) => p.id === participant.id) ||
          participant;
      $('host').textContent = `Host: ${snapshot.host_name}`;
      $('status').textContent =
        `${mode === 'personal' ? 'Remote control' : 'Group control'} · Connected`;
      $('controls').hidden = false;
      render();
      if (queueRevision !== snapshot.queue_revision && !queueLoading)
        void loadQueue(0);
    } else if (message.type === 'reply') {
      const entry = pending.get(message.id);
      if (!entry) return;
      clearTimeout(entry.timer);
      pending.delete(message.id);
      if (message.result.type === 'error')
        entry.reject(new Error(message.result.message));
      else entry.resolve(message.result);
    } else if (message.type === 'host_away') {
      hostLost();
      discard();
      queueRevision = -1;
      $('status').textContent = 'Host disconnected. Waiting to reconnect…';
    } else if (['ended', 'expired', 'error'].includes(message.type)) {
      stopped = true;
      hostReturned();
      expired = message.type === 'expired';
      fail(
        message.message || 'The host ended sharing. Request a new invitation.',
      );
      socket.close();
    }
  };
  socket.onclose = () => {
    discard();
    queueRevision = -1;
    if (!stopped) {
      hostLost();
      $('status').textContent = 'Connection lost. Reconnecting…';
      reconnectTimer = setTimeout(connect, 3000);
    } else {
      $('connect').hidden = false;
      $('disconnect').hidden = true;
      $('status').textContent = expired
        ? 'Session expired; request a new invitation'
        : 'Disconnected';
    }
  };
  socket.onerror = () =>
    fail(
      'Could not reach the host. Check the address, network, and host sharing status.',
    );
}
function render() {
  const p = participant?.permissions;
  $('title').textContent = snapshot.track?.title || 'No track playing';
  $('artist').textContent = snapshot.track?.artist || '';
  for (const id of ['play', 'pause']) $(id).disabled = !p?.transport;
  for (const id of ['previous', 'next']) $(id).disabled = !p?.skip;
  $('seek').disabled = !p?.seek;
  $('seek').max = String(snapshot.duration || 1);
  $('seek').value = String(snapshot.position);
  $('volume').disabled = !p?.volume;
  $('volume').value = String(snapshot.volume);
  $('search').querySelector('button').disabled = !p?.catalog;
  $('clear').disabled = !p?.queue || !p?.replace_queue;
  $('participants').replaceChildren(
    ...snapshot.participants.map((peer) => {
      const li = document.createElement('li');
      li.textContent = `${peer.name}${peer.connected ? '' : ' (disconnected)'}`;
      return li;
    }),
  );
}
async function action(task) {
  fail('');
  try {
    await task();
  } catch (error) {
    fail(String(error.message || error));
  }
}
async function edit(change) {
  try {
    await request({
      type: 'edit_queue',
      revision:
        change.type === 'insert' ? snapshot.queue_revision : queueRevision,
      edit: change,
    });
  } finally {
    queueRevision = -1;
    await loadQueue(0);
  }
}
function row(track, buttons) {
  const node = document.createElement('article');
  node.className = 'row';
  const title = document.createElement('strong');
  title.textContent = track.title;
  const artist = document.createElement('small');
  artist.textContent = track.artist;
  const actions = document.createElement('div');
  actions.className = 'actions';
  for (const [label, run, enabled] of buttons) {
    const b = document.createElement('button');
    b.textContent = label;
    b.disabled = !enabled;
    b.onclick = () => void action(run);
    actions.append(b);
  }
  node.append(title, artist, actions);
  return node;
}
async function loadQueue(offset) {
  queueLoading = true;
  try {
    const page = await request({ type: 'queue', offset, limit: 50 });
    if (offset && page.revision !== queueRevision) {
      queueRevision = -1;
      return;
    }
    queue = offset ? [...queue, ...page.entries] : page.entries;
    queueRevision = page.revision;
    queueTotal = page.total;
    $('queue').replaceChildren(
      ...queue.map((entry, index) =>
        row(entry.track, [
          [
            'Remove',
            () => edit({ type: 'remove', entry_id: entry.id }),
            participant?.permissions.queue &&
              entry.id !== snapshot.current_entry,
          ],
          [
            'Move up',
            () =>
              edit({
                type: 'move',
                entry_id: entry.id,
                before_id: queue[index - 1]?.id,
              }),
            participant?.permissions.queue && index > 0,
          ],
        ]),
      ),
    );
    $('more-queue').hidden = queue.length >= queueTotal;
  } catch (error) {
    fail(error.message);
  } finally {
    queueLoading = false;
  }
}
async function search(offset) {
  $('search-status').textContent = 'Searching public music…';
  const page = await request(
    browseId
      ? { type: 'browse', browse_id: browseId, offset, limit: 50 }
      : { type: 'search', query: searchQuery, offset, limit: 50 },
  );
  results = offset ? [...results, ...page.tracks] : page.tracks;
  resultTotal = page.total;
  collections = offset
    ? [...collections, ...page.collections]
    : page.collections;
  $('search-status').textContent = `${resultTotal} public tracks`;
  $('results').replaceChildren(
    ...collections.map((collection) =>
      row({ title: collection.title, artist: collection.kind }, [
        [
          'Browse collection',
          async () => {
            browseId = collection.browse_id;
            await search(0);
          },
          participant?.permissions.catalog,
        ],
      ]),
    ),
    ...results.map((track) =>
      row(track, [
        [
          'Play now',
          () => edit({ type: 'insert', tracks: [track], placement: 'now' }),
          participant?.permissions.queue && participant?.permissions.transport,
        ],
        [
          'Play next',
          () => edit({ type: 'insert', tracks: [track], placement: 'next' }),
          participant?.permissions.queue,
        ],
        [
          'Add to queue',
          () => edit({ type: 'insert', tracks: [track], placement: 'end' }),
          participant?.permissions.queue,
        ],
      ]),
    ),
  );
  $('more-results').hidden = results.length + collections.length >= resultTotal;
}
$('connect').onsubmit = (e) => {
  e.preventDefault();
  expired = false;
  hostReturned();
  connect();
};
$('manual').onsubmit = (e) => {
  e.preventDefault();
  void action(async () => {
    const url = new URL($('address').value);
    if (!['http:', 'https:'].includes(url.protocol))
      throw new Error('Use an HTTP or HTTPS invitation.');
    location.href = url.href;
  });
};
$('disconnect').onclick = () => {
  stopped = true;
  hostReturned();
  clearTimeout(reconnectTimer);
  socket?.close(1000, 'Disconnected');
  discard();
};
for (const type of ['play', 'pause', 'next', 'previous'])
  $(type).onclick = () =>
    void action(() => request({ type: 'playback', command: { type } }));
$('seek').onchange = () =>
  void action(() =>
    request({
      type: 'playback',
      command: { type: 'seek', position: Number($('seek').value) },
    }),
  );
$('volume').onchange = () =>
  void action(() =>
    request({
      type: 'playback',
      command: { type: 'volume', volume: Number($('volume').value) },
    }),
  );
$('search').onsubmit = (e) => {
  e.preventDefault();
  searchQuery = $('query').value;
  browseId = '';
  void action(() => search(0));
};
$('more-results').onclick = () =>
  void action(() => search(results.length + collections.length));
$('more-queue').onclick = () => void action(() => loadQueue(queue.length));
$('clear').onclick = () => void action(() => edit({ type: 'clear' }));
