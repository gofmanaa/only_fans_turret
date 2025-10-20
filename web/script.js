class WSController {
    constructor() {
        this.ws = null;
        this.pc = null;
        this.hasControl = false;
        this.user_id = 0;
        this.isRequested = false;

        this.user_id = null;
        this.user_id = localStorage.getItem("user_id") || null;

        this.videoStream = document.getElementById('videoStream');
        this.requestBtn = document.getElementById('requestAccessBtn');
        this.releaseBtn = document.getElementById('releaseBtn');
        this.actionLog = document.getElementById('actionLog');
        this.statusDisplay = document.getElementById('connectionStatus');
        this.bytesReceivedEl = document.getElementById('bytesReceived');

        this.controlBtns = {
            up: document.getElementById('upBtn'),
            down: document.getElementById('downBtn'),
            left: document.getElementById('leftBtn'),
            right: document.getElementById('rightBtn'),
            fire: document.getElementById('fireBtn')
        };

        this.videoControlBtns = {
            start: document.getElementById('startBtn'),
            stop: document.getElementById('stopBtn'),
        };
        this.initializeWebSocket();
        this.setupEventListeners();
    }

    initializeWebSocket() {
        const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
        const wsUrl = `${protocol}//${window.location.host}/ws`;

        this.ws = new WebSocket(wsUrl);

        this.ws.onopen = () => {
            this.log('Connected to server', 'success');
            this.updateStatus('Connected - Ready to request access', 'active');
            this.requestBtn.disabled = false;
            this.sendMessage({ type: 'GetUserId' });
        };

        this.ws.onmessage = (event) => this.handleServerMessage(JSON.parse(event.data));
        this.ws.onclose = () => {
            this.sendMessage({ type: 'UserDisconnected', user_id: this.getUserId() });
            this.log('Disconnected from server', 'error');
            this.updateStatus('Disconnected', 'disconnected');
            this.hasControl = false;
            this.updateControlButtons();

            setTimeout(() => this.initializeWebSocket(), 3000);
        };
        this.ws.onerror = (error) => this.log(`WebSocket error: ${error}`, 'error');
    }

    async startWebRTC() {
        try {
            this.pc = new RTCPeerConnection({
                iceServers: [{ urls: ["stun:stun.l.google.com:19302"] }]
            });

            this.pc.onconnectionstatechange = () => {
                if (this.pc.connectionState === 'connected') {
                    this.updateStatus('Streaming', 'active');
                } else if (['disconnected', 'failed'].includes(this.pc.connectionState)) {
                    this.updateStatus('Disconnected', 'disconnected');
                }
            };

            this.pc.ontrack = e => { this.videoStream.srcObject = e.streams[0]; };

            this.pc.addTransceiver("video", { direction: "recvonly" });
            const offer = await this.pc.createOffer();
            await this.pc.setLocalDescription(offer);

            await new Promise(resolve => {
                if (this.pc.iceGatheringState === 'complete') return resolve();
                this.pc.onicegatheringstatechange = () => {
                    if (this.pc.iceGatheringState === 'complete') resolve();
                };
            });

            console.log("send to server client_id:", this.getUserId())

            const resp = await fetch('/sdp', {
                method: 'POST',
                body: JSON.stringify({
                    sdp: this.pc.localDescription.sdp,
                    type: this.pc.localDescription.type,
                    client_id: this.getUserId(),
                }),
                headers: { 'Content-Type': 'application/json' }
            });

            const text = await resp.text(); // always read raw
            try {
                const answer = JSON.parse(text);
                await this.pc.setRemoteDescription(answer);
            } catch (err) {
                console.error("WebRTC error: server did not return valid JSON", text);
                throw err;
            }

            this.videoControlBtns.start.disabled = true;
            this.videoControlBtns.stop.disabled = false;

            // Start stats polling
            this.startStatsPolling();
        } catch (_error) {
            this.log(`WebRTC connect error`, 'error');
            this.updateStatus('Connection Failed', 'disconnected');
        }
    }

    stopWebRTC() {
        if (this.pc) { this.pc.close(); this.pc = null; }
        this.videoStream.srcObject = null;
        this.updateStatus('Stopped', 'disconnected');
        this.videoControlBtns.start.disabled = false;
        this.videoControlBtns.stop.disabled = true;
    }

    startStatsPolling() {
        if (!this.pc) return;
        let lastBytes = 0;
        setInterval(async () => {
            if (!this.pc) return;
            const stats = await this.pc.getStats();
            stats.forEach(report => {
                if (report.type === "inbound-rtp" && report.kind === "video") {
                    const bytes = report.bytesReceived;
                    //this.bytesReceivedEl.textContent = bytes.toLocaleString();
                    const delta = bytes - lastBytes;
                    if (delta > 0) {
                        const bitrate = (delta * 8 / 1000).toFixed(1); // kbps
                        this.bytesReceivedEl.textContent = `${bitrate} kbps`;
                    }
                    lastBytes = bytes;
                }
            });

        }, 3000);
    }

    setupEventListeners() {
        this.requestBtn.addEventListener('click', () => this.sendRequestAccess());
        this.releaseBtn.addEventListener('click', () => this.sendReleaseControl());
        this.videoControlBtns.start.addEventListener('click', () => this.startWebRTC());
        this.videoControlBtns.stop.addEventListener('click', () => this.stopWebRTC());

        Object.entries(this.controlBtns).forEach(([action, btn]) => {
            btn.addEventListener('click', () => this.sendControlAction(action));
        });

        document.addEventListener('keydown', (event) => {
            if (!this.hasControl) return;
            const keyMap = { ArrowUp:'up', ArrowDown:'down', ArrowLeft:'left', ArrowRight:'right', ' ':'fire', Enter:'fire' };
            if (keyMap[event.key]) {
                event.preventDefault();
                this.sendControlAction(keyMap[event.key]);
            }
        });

        window.addEventListener('beforeunload', () => this.stopWebRTC());
    }

    sendRequestAccess() {
        this.sendMessage({ type: 'RequestAccess' });
        this.isRequested = true;
        this.requestBtn.disabled = true;
        this.requestBtn.textContent = '⏳ Requesting...';
        this.log('Requesting access...', 'info');
    }


    releaseControl() {
        this.hasControl = false;
        this.requestBtn.textContent = '🎯 Request Access';
        this.updateControlButtons();
        this.updateStatus('Released control', 'waiting');
        this.log('Released control', 'info');
    }

    sendReleaseControl() {
        this.releaseControl()
        this.sendMessage({ type: 'ReleaseControl' });
    }

    sendControlAction(action) {
        if (!this.hasControl) return;
        this.sendMessage({ type: 'Control', action: action.toUpperCase() });
        this.log(`Sent command: ${action.toUpperCase()}`, 'info');
    }

    sendMessage(message) {
        if (this.ws.readyState === WebSocket.OPEN) {
            console.log("Message: ", message);
            this.ws.send(JSON.stringify(message));
        }
    }

    updateStatus(text, type) {
        this.statusDisplay.textContent = text;
        this.statusDisplay.className = `status ${type}`;
    }

    updateControlButtons() {
        const isEnabled = this.hasControl;
        Object.values(this.controlBtns).forEach(btn => btn.disabled = !isEnabled);
        this.requestBtn.style.display = isEnabled ? 'none' : 'block';
        this.releaseBtn.style.display = isEnabled ? 'block' : 'none';
        this.requestBtn.disabled = isEnabled;
        this.releaseBtn.disabled = !isEnabled;
    }

    log(message, type = 'info') {
        const timestamp = new Date().toLocaleTimeString();
        const logEntry = document.createElement('div');
        logEntry.className = `log-entry ${type}`;
        logEntry.textContent = `[${timestamp}] ${message}`;
        this.actionLog.appendChild(logEntry);
        this.actionLog.scrollTop = this.actionLog.scrollHeight;
        while (this.actionLog.children.length > 50) {
            this.actionLog.removeChild(this.actionLog.firstChild);
        }
    }

    getUserId() {
        console.log("getUserId: ", this.user_id);
        return this.user_id;
    }
    setUserId(id) {
        this.user_id = id;
        localStorage.setItem("user_id", id);
        console.log("setUserId: ", id);
    }

    handleServerMessage(msg) {

        console.log("server message: ", msg);

        if (msg.type !== 'ResponseUserId') {
            if (msg.user_id && msg.user_id !== this.getUserId()) {
                return;
            }
        }

        switch (msg.type) {
            case 'ResponseUserId':
                this.setUserId(msg.user_id);
                this.log(`User ID: ${msg.user_id}`, 'success');
                break;
            case 'AccessGranted':
                this.hasControl = true;
                this.updateControlButtons();
                this.updateStatus('🎮 You have control!', 'active');
                this.log('Access granted!', 'success');
                break;
            case 'AccessDenied':
                this.releaseControl();
                this.updateStatus('No access', 'disconnected');
                this.log('Access denied!', 'error');
                break;
            case 'QueuePosition':
                this.updateStatus(`⏳ Queue Position: ${msg.position}`, 'waiting');
                this.log(`Your position in queue: ${msg.position}`, 'info');
                break;
            case 'ControlAction':
                // This message is typically for the controlling user to confirm their action,
                // or for all users to see the action. Given the server-side change,
                // it's now targeted.
                this.log(`Action: ${msg.action}`, 'info');
                break;
            case 'Error':
                this.log(`Error: ${msg.message}`, 'error');
                break;
            case 'UserDisconnected':
                this.log(`User ${msg.user_id} disconnected.`, 'info');
                break;
            default:
                console.log(`Unknown message: ${msg.type}`, 'warning');
        }
    }
}

document.addEventListener('DOMContentLoaded', () => new WSController());