// wir.py が評価の前に読み込む、画面を操作する小さな関数。
window.__h = {
	/** 文言を含むボタン・リンクのうち、最後のものを押す。見つからなければ false。 */
	click: function (text) {
		var found = [].slice
			.call(document.querySelectorAll('button,a,[role=menuitem],[role=tab],label'))
			.filter(function (el) {
				return el.textContent.trim().indexOf(text) >= 0;
			});
		if (!found.length) return false;
		found[found.length - 1].click();
		return true;
	},
	/** 入力欄 (input・textarea・select) に値を入れる。Svelte の bind に届くよう input イベントを送る。見つからなければ false。 */
	fill: function (id, value) {
		var el = document.getElementById(id);
		if (!el) return false;
		Object.getOwnPropertyDescriptor(Object.getPrototypeOf(el), 'value').set.call(el, value);
		el.dispatchEvent(new Event('input', { bubbles: true }));
		return true;
	},
	/** 画面にある `index` 番目の input[type=file] で、`url` のファイルを選んだことにする。
	 * iOS の写真・ファイルの選択画面は JS から操作できないため、その先の流れを確かめるのに使う。 */
	pick: function (url, name, type, index) {
		return fetch(url)
			.then(function (r) {
				return r.blob();
			})
			.then(function (blob) {
				var input = document.querySelectorAll('input[type=file]')[index || 0];
				var dt = new DataTransfer();
				dt.items.add(new File([blob], name, { type: type }));
				input.files = dt.files;
				input.dispatchEvent(new Event('change', { bubbles: true }));
			});
	},
	/** SvelteKit のルーターで移る (一時的なリンクを押す。history.pushState では画面が変わらない)。 */
	go: function (path) {
		var a = document.createElement('a');
		a.href = path;
		document.body.appendChild(a);
		a.click();
		a.remove();
	},
	wait: function (ms) {
		return new Promise(function (resolve) {
			setTimeout(resolve, ms);
		});
	}
};
