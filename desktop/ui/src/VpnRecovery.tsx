import { useEffect, useRef, useState } from "react";

import guide from "../../../docs/remote.md?raw";
import { nodes, type Started } from "./bridge";
import { renderMarkdown } from "./markdown";

const preference = "ragon.connectThroughVpn";
type Phase = "idle" | "checking" | "connecting" | "unreachable" | "unenrolled" | "down" | "missing" | "saving";

export function VpnRecovery(props: { exited: boolean; log: string[]; connect: (running?: Started) => void }) {
  const [phase, setPhase] = useState<Phase>("idle");
  const [error, setError] = useState("");
  const [address, setAddress] = useState("");
  const attempted = useRef(false);
  const busy = useRef(false);
  const missed = props.log.some(line => /No Ragon node (found|answered) on this network|did not answer a browse of this network|no such node on this network|The remembered node, .+, is not on this network/i.test(line));

  async function connectVpn() {
    if (busy.current) return;
    busy.current = true;
    attempted.current = true;
    setPhase("checking");
    setError("");
    try {
      localStorage.setItem(preference, "yes");
      const result = await nodes.vpn();
      if (result.state === "ready" || result.state === "discover") {
        setPhase("connecting");
        props.connect(result.started);
      } else {
        setPhase(result.state);
      }
    } catch {
      setPhase("idle");
      setError("Could not check the VPN connection. Try again.");
    } finally {
      busy.current = false;
    }
  }

  useEffect(() => {
    if (props.exited && missed && !attempted.current && localStorage.getItem(preference) === "yes") void connectVpn();
  }, [props.exited, missed]);

  async function saveAddress() {
    if (busy.current) return;
    busy.current = true;
    setPhase("saving");
    setError("");
    try {
      if (await nodes.address(address.trim())) {
        localStorage.removeItem(preference);
        props.connect();
      }
    } catch {
      setError("Could not use that address. Enter a hostname or IP address, optionally followed by a port.");
    } finally {
      setPhase("unenrolled");
      busy.current = false;
    }
  }

  if (!props.exited && phase !== "connecting") return null;
  return <>
    {props.exited && <button className="primary" disabled={phase === "checking" || phase === "saving"} onClick={() => props.connect()}>Start again</button>}
    {(missed || phase !== "idle") && <VpnControls phase={props.exited && phase === "connecting" ? "unreachable" : phase} error={error} address={address} onAddress={setAddress} onSave={saveAddress} onVpn={connectVpn} />}
  </>;
}

export function VpnControls(props: {
  phase: Phase; error: string; address: string; onAddress: (value: string) => void; onSave: () => void; onVpn: () => void;
}) {
  const [guideOpen, setGuideOpen] = useState(false);
  if (props.phase === "connecting") return <p role="status">Connecting to your node through VPN…</p>;
  const retry = ["down", "missing", "unreachable"].includes(props.phase);
  const entry = retry || props.phase === "unenrolled" || props.phase === "saving";
  return <>
    <button disabled={props.phase === "checking" || props.phase === "saving"} onClick={props.onVpn}>
      {retry ? "Retry VPN connection" : "Connect through VPN"}
    </button>
    {props.phase === "checking" && <p role="status">Checking the VPN connection…</p>}
    {props.phase === "unreachable" && <p role="status">The VPN is connected, but the node did not answer. Check the node and retry, or enter its address.</p>}
    {props.phase === "down" && <p role="status">NetBird is disconnected. Connect it, then retry.</p>}
    {props.phase === "missing" && <p role="status">NetBird could not be found. Check the installation or follow the enrolment guide.</p>}
    {entry && <div>
      <p>{retry ? "If this computer still needs enrolment, join once on your node’s home network." : "Enrol this computer once at home, on your node’s network."}<br />
        Follow the <a href="#vpn-enrolment" onClick={event => { event.preventDefault(); setGuideOpen(!guideOpen); }}>remote access guide</a>, then connect through VPN.</p>
      {guideOpen && <section id="vpn-enrolment" className="markdown" dangerouslySetInnerHTML={{ __html: renderMarkdown(
        guide.match(/^## How do I join a laptop on the same network\?[\s\S]*?(?=^## )/m)?.[0] ?? guide
      ) }} />}
      <form onSubmit={event => { event.preventDefault(); props.onSave(); }}>
        <label>Node address <input value={props.address} onChange={event => props.onAddress(event.target.value)} /></label>
        <button disabled={!props.address.trim() || props.phase === "saving"}>Use address</button>
      </form>
      {props.phase === "saving" && <p role="status">Saving the node address…</p>}
    </div>}
    {props.error && <p role="alert">{props.error}</p>}
  </>;
}
