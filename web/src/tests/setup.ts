import '@testing-library/jest-dom/vitest';

if (typeof Element.prototype.animate === 'undefined') {
	Element.prototype.animate = function () {
		return {
			cancel: () => {},
			finish: () => {},
			pause: () => {},
			play: () => {},
			reverse: () => {},
			onfinish: null,
			oncancel: null,
			finished: Promise.resolve(),
			currentTime: 0,
			playbackRate: 1,
			effect: null,
			timeline: null,
			id: '',
			pending: false,
			playState: 'finished',
			ready: Promise.resolve(),
			startTime: null,
			persist: () => {},
			commitStyles: () => {},
			addEventListener: () => {},
			removeEventListener: () => {},
			dispatchEvent: () => false,
			replaceState: () => {},
			updatePlaybackRate: () => {}
		} as unknown as Animation;
	};
}
