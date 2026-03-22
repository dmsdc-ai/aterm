import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const xtermDir = path.resolve(__dirname, '../node_modules/@xterm/xterm');
const pkgPath = path.join(xtermDir, 'package.json');
const bundlePath = path.join(xtermDir, 'lib/xterm.mjs');

if (!fs.existsSync(pkgPath) || !fs.existsSync(bundlePath)) {
  console.warn('[patch-xterm-wk-ime] @xterm/xterm not installed, skipping');
  process.exit(0);
}

const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));
if (pkg.version !== '6.1.0-beta.195') {
  throw new Error(`[patch-xterm-wk-ime] Expected @xterm/xterm@6.1.0-beta.195, found ${pkg.version}`);
}

function replaceOnce(source, oldText, newText, label) {
  const index = source.indexOf(oldText);
  if (index === -1) {
    throw new Error(`[patch-xterm-wk-ime] Could not find ${label}`);
  }
  return source.slice(0, index) + newText + source.slice(index + oldText.length);
}

function replaceSection(source, startMarker, endMarker, newText, label) {
  const start = source.indexOf(startMarker);
  if (start === -1) {
    throw new Error(`[patch-xterm-wk-ime] Could not find ${label} start`);
  }
  const end = source.indexOf(endMarker, start);
  if (end === -1) {
    throw new Error(`[patch-xterm-wk-ime] Could not find ${label} end`);
  }
  return source.slice(0, start) + newText + source.slice(end + 1);
}

let source = fs.readFileSync(bundlePath, 'utf8');

const oldPatchedKeydown =
  '_keyDown(e){if(this._keyDownHandled=!1,this._keyDownSeen=!0,this.browser.isSafari&&e.keyCode===229&&!this.optionsService.rawOptions.screenReaderMode&&this._wkSetComposing(!0),this._wkImeComposing&&e.keyCode!==229&&this._wkFlush(),this._customKeyEventHandler&&this._customKeyEventHandler(e)===!1)return!1;let i=this.browser.isMac&&this.options.macOptionIsMeta&&e.altKey;if(!i&&!this._compositionHelper.keydown(e))return this.options.scrollOnUserInput&&this.buffer.ybase!==this.buffer.ydisp&&this.scrollToBottom(!0),!1;!i&&(e.key==="Dead"||e.key==="AltGraph")&&(this._unprocessedDeadKey=!0);let r=this._keyboardService.evaluateKeyDown(e);if(this.updateCursorStyle(e),r.type===3||r.type===2){let o=this.rows-1;return this.scrollLines(r.type===2?-o:o),e.preventDefault(),e.stopPropagation(),!1}if(r.type===1&&this.selectAll(),this._isThirdLevelShift(this.browser,e)||(r.cancel&&(e.preventDefault(),e.stopPropagation()),!r.key)||!this._keyboardService.useKitty&&!this._keyboardService.useWin32InputMode&&e.key&&!e.ctrlKey&&!e.altKey&&!e.metaKey&&e.key.length===1&&e.key.charCodeAt(0)>=65&&e.key.charCodeAt(0)<=90)return!0;if(this._unprocessedDeadKey)return this._unprocessedDeadKey=!1,!0;(r.key===""||r.key==="\\r")&&(this.textarea.value="");let s=this._keyboardService.useWin32InputMode&&Bs(e);if(this._onKey.fire({key:r.key,domEvent:e}),this._showCursor(),this.coreService.triggerDataEvent(r.key,!s),!this.optionsService.rawOptions.screenReaderMode||e.altKey||e.ctrlKey)return e.preventDefault(),e.stopPropagation(),!1;this._keyDownHandled=!0}';
const newPatchedKeydown =
  '_keyDown(e){if(this._keyDownHandled=!1,this._keyDownSeen=!0,this.browser.isSafari&&e.keyCode===229&&!this.optionsService.rawOptions.screenReaderMode&&this._wkSetComposing(!0),this._wkImeComposing&&e.keyCode!==229&&e.keyCode!==16&&e.keyCode!==17&&e.keyCode!==18&&e.keyCode!==20&&e.keyCode!==91&&e.keyCode!==93&&e.keyCode!==224&&this._wkFlush(),this._customKeyEventHandler&&this._customKeyEventHandler(e)===!1)return!1;let i=this.browser.isMac&&this.options.macOptionIsMeta&&e.altKey;if(!i&&!this._compositionHelper.keydown(e))return this.options.scrollOnUserInput&&this.buffer.ybase!==this.buffer.ydisp&&this.scrollToBottom(!0),!1;!i&&(e.key==="Dead"||e.key==="AltGraph")&&(this._unprocessedDeadKey=!0);let r=this._keyboardService.evaluateKeyDown(e);if(this.updateCursorStyle(e),r.type===3||r.type===2){let o=this.rows-1;return this.scrollLines(r.type===2?-o:o),e.preventDefault(),e.stopPropagation(),!1}if(r.type===1&&this.selectAll(),this._isThirdLevelShift(this.browser,e)||(r.cancel&&(e.preventDefault(),e.stopPropagation()),!r.key)||!this._keyboardService.useKitty&&!this._keyboardService.useWin32InputMode&&e.key&&!e.ctrlKey&&!e.altKey&&!e.metaKey&&e.key.length===1&&e.key.charCodeAt(0)>=65&&e.key.charCodeAt(0)<=90)return!0;if(this._unprocessedDeadKey)return this._unprocessedDeadKey=!1,!0;(r.key===""||r.key==="\\r")&&(this.textarea.value="");let s=this._keyboardService.useWin32InputMode&&Bs(e);if(this._onKey.fire({key:r.key,domEvent:e}),this._showCursor(),this.coreService.triggerDataEvent(r.key,!s),!this.optionsService.rawOptions.screenReaderMode||e.altKey||e.ctrlKey)return e.preventDefault(),e.stopPropagation(),!1;this._keyDownHandled=!0}';

const isPatched =
  source.includes('this._wkImeComposing=!1;this._wkImePending="";this._wkImeLastFlushed="";this._accessibilityManager=') &&
  source.includes('!this.wkImeComposing&&this._handleAnyTextareaChanges()') &&
  source.includes(newPatchedKeydown);

if (isPatched) {
  console.log('[patch-xterm-wk-ime] lib/xterm.mjs already patched');
  process.exit(0);
}

if (source.includes(oldPatchedKeydown)) {
  source = replaceOnce(source, oldPatchedKeydown, newPatchedKeydown, 'CoreBrowserTerminal keydown upgrade');
  fs.writeFileSync(bundlePath, source);
  console.log('[patch-xterm-wk-ime] upgraded existing lib/xterm.mjs patch');
  process.exit(0);
}

if (source.includes('WKWebView Hangul pending buffer patch') || source.includes('WKWebView Hangul composition emulator patch')) {
  throw new Error('[patch-xterm-wk-ime] Found an older local IME patch. Restore a clean @xterm/xterm install before rerunning this patch.');
}

source = replaceOnce(
  source,
  'this._keyPressHandled=!1;this._unprocessedDeadKey=!1;this._accessibilityManager=',
  'this._keyPressHandled=!1;this._unprocessedDeadKey=!1;this._wkImeComposing=!1;this._wkImePending="";this._wkImeLastFlushed="";this._accessibilityManager=',
  'CoreBrowserTerminal constructor state'
);

source = replaceOnce(
  source,
  'this._onBlur.fire()}_syncTextArea(){',
  'this._onBlur.fire()}static _isHangul(t){let e=t.codePointAt(0);return e!==void 0&&((e>=4352&&e<=4607)||(e>=12592&&e<=12687)||(e>=44032&&e<=55215)||(e>=43360&&e<=43391)||(e>=55216&&e<=55295))}_wkShowComposition(t){if(!this._compositionView||!this._renderService)return;this._compositionView.textContent=t,this._compositionView.classList.add("active");let e=Math.min(this.buffer.x,this.cols-1),i=this._renderService.dimensions.css.cell.height,r=this.buffer.y*i,s=e*this._renderService.dimensions.css.cell.width;this._compositionView.style.left=s+"px",this._compositionView.style.top=r+"px",this._compositionView.style.height=i+"px",this._compositionView.style.lineHeight=i+"px",this._compositionView.style.fontFamily=this.optionsService.rawOptions.fontFamily??"",this._compositionView.style.fontSize=this.optionsService.rawOptions.fontSize+"px"}_wkHideComposition(){this._compositionView&&(this._compositionView.textContent="",this._compositionView.classList.remove("active"))}_wkSetComposing(t){this._wkImeComposing=t,this._compositionHelper&&(this._compositionHelper.wkImeComposing=t)}_wkFlush(){if(!this._wkImeComposing)return;let t=this._wkImePending;this._wkSetComposing(!1),this._wkImePending="",this._wkHideComposition(),this._wkImeLastFlushed=t,t&&this.coreService.triggerDataEvent(t,!0)}_syncTextArea(){',
  'CoreBrowserTerminal WKWebView helpers'
);

source = replaceOnce(
  source,
  'this._isComposing=!1,this._isSendingComposition=!1,this._compositionPosition={start:0,end:0},this._compositionSuffix="",this._dataAlreadySent=""}get isComposing(){return this._isComposing}compositionstart(){',
  'this._isComposing=!1,this._isSendingComposition=!1,this.wkImeComposing=!1,this._compositionPosition={start:0,end:0},this._compositionSuffix="",this._dataAlreadySent=""}get isComposing(){return this._isComposing}compositionstart(){',
  'CompositionHelper state'
);

source = replaceOnce(
  source,
  'keydown(t){if(this._isComposing||this._isSendingComposition){if(t.keyCode===20||t.keyCode===229||t.keyCode===16||t.keyCode===17||t.keyCode===18)return!1;this._finalizeComposition(!1)}return t.keyCode===229?(this._handleAnyTextareaChanges(),!1):!0}',
  'keydown(t){if(this._isComposing||this._isSendingComposition){if(t.keyCode===20||t.keyCode===229||t.keyCode===16||t.keyCode===17||t.keyCode===18)return!1;this._finalizeComposition(!1)}return t.keyCode===229?(!this.wkImeComposing&&this._handleAnyTextareaChanges(),!1):!0}',
  'CompositionHelper keydown'
);

source = replaceSection(
  source,
  '_keyDown(e){',
  '}_isThirdLevelShift',
  newPatchedKeydown,
  'CoreBrowserTerminal keydown'
);

source = replaceSection(
  source,
  '_inputEvent(e){',
  '}resize(e,i){',
  '_inputEvent(e){if(e.data&&e.inputType==="insertReplacementText"&&!this.optionsService.rawOptions.screenReaderMode){if(this._wkImeLastFlushed&&e.data===this._wkImeLastFlushed)return this._wkImeLastFlushed="",e.preventDefault(),e.stopPropagation(),!0;return this._wkSetComposing(!0),this._wkImePending=e.data,this._wkShowComposition(e.data),e.preventDefault(),e.stopPropagation(),!0}if(e.data&&e.inputType==="insertText"&&this.constructor._isHangul(e.data)&&!this.optionsService.rawOptions.screenReaderMode){if(this._wkImeLastFlushed&&e.data===this._wkImeLastFlushed)return this._wkImeLastFlushed="",e.preventDefault(),e.stopPropagation(),!0;let i=this._wkImePending.length>0;return this._wkFlush(),this._wkSetComposing(!0),this._wkImePending=e.data,i||this._wkShowComposition(e.data),e.preventDefault(),e.stopPropagation(),!0}if(e.data&&e.inputType==="insertText"&&(!e.composed||!this._keyDownSeen)&&!this.optionsService.rawOptions.screenReaderMode){if(this._wkImeLastFlushed){if(e.data===this._wkImeLastFlushed)return this._wkImeLastFlushed="",!0;this._wkImeLastFlushed=""}if(this._keyPressHandled)return!1;this._wkFlush(),this._unprocessedDeadKey=!1;let i=e.data;return this.coreService.triggerDataEvent(i,!0),!0}return!1}',
  'CoreBrowserTerminal inputEvent'
);

fs.writeFileSync(bundlePath, source);
console.log('[patch-xterm-wk-ime] patched lib/xterm.mjs');
