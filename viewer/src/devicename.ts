export function deviceLabel(userAgent: string, maxTouchPoints = 0): string {
  const agent = userAgent.toLowerCase();
  let system = '';
  if (/iphone/u.test(agent)) {
    system = 'iPhone';
  } else if (/ipad/u.test(agent) || (/macintosh/u.test(agent) && maxTouchPoints > 1)) {
    system = 'iPad';
  } else if (/android/u.test(agent)) {
    system = 'Android';
  } else if (/windows/u.test(agent)) {
    system = 'Windows';
  } else if (/macintosh|mac os x/u.test(agent)) {
    system = 'Mac';
  } else if (/cros/u.test(agent)) {
    system = 'ChromeOS';
  } else if (/linux|x11/u.test(agent)) {
    system = 'Linux';
  }
  let browser = '';
  if (/edg(e|a|ios)?\//u.test(agent)) {
    browser = 'Edge';
  } else if (/opr\/|opera/u.test(agent)) {
    browser = 'Opera';
  } else if (/firefox|fxios/u.test(agent)) {
    browser = 'Firefox';
  } else if (/chrome|crios/u.test(agent)) {
    browser = 'Chrome';
  } else if (/safari/u.test(agent)) {
    browser = 'Safari';
  }
  return [system, browser].filter((part) => part !== '').join(' - ');
}
