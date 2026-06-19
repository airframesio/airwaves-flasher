import { useEffect, useMemo, useState } from 'react';
import { Info, ShieldAlert } from 'lucide-react';
import { useFlasher } from '../store';
import type { Channel } from '../types';
import { detectSshKeys } from '../lib/api';
import {
  guessTimezone,
  isValidHostname,
  isValidSshKey,
  timezones,
} from '../lib/format';
import { Select } from '../components/Select';

const CHANNELS: Channel[] = ['stable', 'beta', 'dev'];

/** Split a key so the comment/tail stays visible while the middle ellipsizes. */
function SshKeyLabel({ value }: { value: string }) {
  const tail = value.length > 16 ? value.slice(-14) : '';
  const main = tail ? value.slice(0, -14) : value;
  return (
    <span className="sshkey" title={value}>
      <span className="sshkey__main">{main}</span>
      {tail && <span className="sshkey__tail">{tail}</span>}
    </span>
  );
}

function Switch({ on, onClick }: { on: boolean; onClick: () => void }) {
  return (
    <button
      className={`switch ${on ? 'is-on' : ''}`}
      role="switch"
      aria-checked={on}
      onClick={onClick}
    />
  );
}

export function CustomizeStep() {
  const { customization, setCustomization, source } = useFlasher();
  const c = customization;
  const tzList = useMemo(() => timezones(), []);
  const [detected, setDetected] = useState<string[]>([]);
  const [manual, setManual] = useState('');

  // sensible defaults the first time the step is opened
  useEffect(() => {
    if (!c.timezone) setCustomization({ timezone: guessTimezone() });
    if (!c.channel && source?.kind === 'catalog')
      setCustomization({ channel: source.image.channel });
    detectSshKeys().then(setDetected).catch(() => setDetected([]));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function recomputeKeys(nextDetected: string[], manualText: string) {
    const manualKeys = manualText
      .split('\n')
      .map((l) => l.trim())
      .filter((l) => l && isValidSshKey(l));
    setCustomization({ sshKeys: [...nextDetected, ...manualKeys] });
  }

  const [pickedDetected, setPickedDetected] = useState<Set<string>>(new Set());

  function toggleDetected(key: string) {
    const next = new Set(pickedDetected);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    setPickedDetected(next);
    recomputeKeys([...next], manual);
  }

  const hostnameError = c.hostname && !isValidHostname(c.hostname);

  return (
    <div className="stage__inner">
      <div className="stage__head">
        <span className="kicker">Step 3 — First boot (optional)</span>
        <h1 className="stage__title">Pre-configure the device</h1>
        <p className="stage__lede">
          Write settings to the card so a headless device comes online ready — no screen or
          keyboard needed. These are applied automatically on first boot.
        </p>
      </div>

      <div className="switchrow" style={{ paddingTop: 0 }}>
        <div className="switchrow__text">
          <strong>Pre-configure first boot</strong>
          <p>
            Set hostname, Wi-Fi, time zone, update channel and SSH keys. Leave off to flash a
            clean image and configure on the device’s web dashboard.
          </p>
        </div>
        <Switch on={c.enabled} onClick={() => setCustomization({ enabled: !c.enabled })} />
      </div>

      {c.enabled && (
        <div className="subform">
          {/* hostname */}
          <div className="field">
            <label className="field__label" htmlFor="hostname">
              Hostname
            </label>
            <input
              id="hostname"
              className="input"
              placeholder="airwaves-feeder"
              value={c.hostname}
              spellCheck={false}
              autoCapitalize="off"
              onChange={(e) => setCustomization({ hostname: e.target.value })}
            />
            <div className={`field__hint ${hostnameError ? 'is-error' : ''}`}>
              {hostnameError
                ? 'Use only lowercase letters, digits and hyphens (max 63 chars).'
                : 'Reachable at hostname.local on your network. Leave blank to auto-name from MAC.'}
            </div>
          </div>

          {/* wifi */}
          <div className="switchrow">
            <div className="switchrow__text">
              <strong>Configure Wi-Fi</strong>
              <p>Connect over wireless on first boot. Skip this for Ethernet.</p>
            </div>
            <Switch
              on={c.wifiEnabled}
              onClick={() => setCustomization({ wifiEnabled: !c.wifiEnabled })}
            />
          </div>
          {c.wifiEnabled && (
            <>
              <div className="formgrid">
                <div className="field">
                  <label className="field__label" htmlFor="ssid">
                    Network name (SSID)
                  </label>
                  <input
                    id="ssid"
                    className="input"
                    value={c.wifi.ssid}
                    spellCheck={false}
                    onChange={(e) =>
                      setCustomization({ wifi: { ...c.wifi, ssid: e.target.value } })
                    }
                  />
                </div>
                <div className="field">
                  <label className="field__label" htmlFor="psk">
                    Password
                  </label>
                  <input
                    id="psk"
                    className="input"
                    type="password"
                    value={c.wifi.psk}
                    placeholder="leave blank for open network"
                    onChange={(e) =>
                      setCustomization({ wifi: { ...c.wifi, psk: e.target.value } })
                    }
                  />
                </div>
              </div>
              <div className="banner banner--warn" style={{ marginBottom: '0.6rem' }}>
                <ShieldAlert size={16} />
                <div>
                  The Wi-Fi password is stored in plain text on the card’s config partition. Keep
                  the card physically secure.
                </div>
              </div>
            </>
          )}

          {/* channel + timezone */}
          <div className="formgrid">
            <div className="field">
              <label className="field__label" htmlFor="channel">
                Update channel
              </label>
              <Select
                id="channel"
                ariaLabel="Update channel"
                value={c.channel || 'stable'}
                options={CHANNELS.map((ch) => ({ value: ch, label: ch }))}
                onChange={(v) => setCustomization({ channel: v as Channel })}
              />
              <div className="field__hint">Which channel the device pulls OS updates from.</div>
            </div>
            <div className="field">
              <label className="field__label" htmlFor="tz">
                Time zone
              </label>
              <Select
                id="tz"
                ariaLabel="Time zone"
                searchable
                placeholder="Select time zone"
                value={c.timezone}
                options={tzList.map((tz) => ({ value: tz, label: tz }))}
                onChange={(v) => setCustomization({ timezone: v })}
              />
            </div>
          </div>

          {/* ssh */}
          <div className="switchrow">
            <div className="switchrow__text">
              <strong>Authorize SSH keys</strong>
              <p>Add public keys for passwordless remote login as root and airwaves.</p>
            </div>
            <Switch
              on={c.sshEnabled}
              onClick={() => setCustomization({ sshEnabled: !c.sshEnabled })}
            />
          </div>
          {c.sshEnabled && (
            <div style={{ marginTop: '0.4rem' }}>
              {detected.length > 0 && (
                <div style={{ marginBottom: '0.6rem' }}>
                  <div className="field__label">Detected on this computer</div>
                  {detected.map((k) => (
                    <label key={k} className="checkrow">
                      <input
                        type="checkbox"
                        checked={pickedDetected.has(k)}
                        onChange={() => toggleDetected(k)}
                      />
                      <SshKeyLabel value={k} />
                    </label>
                  ))}
                </div>
              )}
              <div className="field" style={{ marginBottom: 0 }}>
                <label className="field__label" htmlFor="keys">
                  Add public keys (one per line)
                </label>
                <textarea
                  id="keys"
                  className="textarea"
                  placeholder="ssh-ed25519 AAAA… you@host"
                  value={manual}
                  spellCheck={false}
                  onChange={(e) => {
                    setManual(e.target.value);
                    recomputeKeys([...pickedDetected], e.target.value);
                  }}
                />
              </div>
            </div>
          )}
        </div>
      )}

      <div className="banner banner--info" style={{ marginTop: '1.3rem' }}>
        <Info size={18} />
        <div>
          Station identity — operator name, location and which aggregators you feed — is set up
          afterwards in the device’s web dashboard, the first time you open it.
        </div>
      </div>
    </div>
  );
}
