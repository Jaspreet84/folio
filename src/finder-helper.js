// Launch Services helper for folio's Finder integration, run with
// `osascript -l JavaScript`. It uses NSWorkspace, so it needs no Xcode.
//
//   get FILE             -> "<app path>\t<bundle id>" of the app that opens FILE (empty if none)
//   set FILE APP [SECS]  -> "ok" once APP opens files like FILE, else "unchanged"
//                           (macOS 26.4 and later ask the user first; "Keep" leaves it unchanged)
//   app BUNDLE_ID        -> "<app path>\t<version>" of that app (empty if not installed)
ObjC.import('AppKit');

function bundleId(url) {
  if (url.isNil()) return '';
  var b = $.NSBundle.bundleWithURL(url);
  return (b.isNil() || b.bundleIdentifier.isNil()) ? '' : ObjC.unwrap(b.bundleIdentifier);
}

function run(argv) {
  var ws = $.NSWorkspace.sharedWorkspace;
  if (argv[0] === 'app') {
    var url = ws.URLForApplicationWithBundleIdentifier($(argv[1]));
    if (url.isNil()) return '';
    var path = ObjC.unwrap(url.path), b = $.NSBundle.bundleWithURL(url);
    if (b.isNil()) return path + '\t';
    var v = b.objectForInfoDictionaryKey($('CFBundleShortVersionString'));
    return path + '\t' + (v.isNil() ? '' : ObjC.unwrap(v));
  }
  var file = $.NSURL.fileURLWithPath($(argv[1]));
  if (argv[0] === 'get') {
    var cur = ws.URLForApplicationToOpenURL(file);
    return cur.isNil() ? '' : ObjC.unwrap(cur.path) + '\t' + bundleId(cur);
  }
  if (argv[0] === 'set') {
    var app = $.NSURL.fileURLWithPath($(argv[2])), want = bundleId(app);
    ws.setDefaultApplicationAtURLToOpenContentTypeOfFileAtURLCompletionHandler(app, file, null);
    var deadline = Date.now() + 1000 * Number(argv[3] || 120);
    while (Date.now() < deadline) {
      $.NSRunLoop.currentRunLoop.runUntilDate($.NSDate.dateWithTimeIntervalSinceNow(0.25));
      if (bundleId(ws.URLForApplicationToOpenURL(file)) === want) return 'ok';
    }
    return 'unchanged';
  }
  throw new Error('usage: get FILE | set FILE APP [SECS] | app BUNDLE_ID');
}
