class WSController {
    constructor() {
        this.ws = null;
        this.pc = null;
        this.hasControl = false;
        this.user_id = 0;
        this.position = 0;

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
            this.position = 0;
            this.updateControlButtons();

            setTimeout(() => this.initializeWebSocket(), 3000);
        };
        this.ws.onerror = (event) => {
            this.log(`WebSocket closed`, 'error')
            this.position = 0;
            console.error('WebSocket error event:', event);
        };
    }

    async startWebRTC() {
        try {
            console.log("Getting TURN credentials...");

            // Get TURN credentials first
            const turnResp = await fetch("/turn", {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({ client_id: this.getUserId() }),
            });
            const turnData = await turnResp.json();

            console.log("Creating peer connection with TURN...");

            // Close old connection if exists
            if (this.pc) this.pc.close();

            // Create peer connection with TURN from the start
            this.pc = new RTCPeerConnection({
                iceServers: [{
                    urls: turnData.turn.urls,
                    username: turnData.turn.username,
                    credential: turnData.turn.credential,
                }],
            });

            this.pc.onconnectionstatechange = () => {
                switch (this.pc.connectionState) {
                    case "connected":
                        this.updateStatus("Streaming", "active");
                        break;
                    case "disconnected":
                    case "failed":
                    case "closed":
                        this.updateStatus("Disconnected", "disconnected");
                        break;
                }
            };

            this.pc.ontrack = (event) => {
                this.videoStream.srcObject = event.streams[0];
            };

            this.pc.addTransceiver("video", { direction: "recvonly" });

            const offer = await this.pc.createOffer();
            await this.pc.setLocalDescription(offer);

            await new Promise(resolve => {
                if (this.pc.iceGatheringState === "complete") return resolve();
                this.pc.onicegatheringstatechange = () => {
                    if (this.pc.iceGatheringState === "complete") resolve();
                };
            });

            const resp = await fetch("/sdp", {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({
                    sdp: this.pc.localDescription.sdp,
                    type: this.pc.localDescription.type,
                    client_id: this.getUserId(),
                }),
            });

            const json = await resp.json();
            if (!resp.ok) throw new Error(json.error || "SDP error");

            await this.pc.setRemoteDescription({
                type: json.type,
                sdp: json.sdp,
            });

            this.videoControlBtns.start.disabled = true;
            this.videoControlBtns.stop.disabled = false;
            this.startStatsPolling();

            console.log("WebRTC connected!");
        } catch (error) {
            console.error("WebRTC failed:", error);
            this.updateStatus('Connection Failed', 'disconnected');
        }
    }

    stopWebRTC() {
        if (this.pc) { this.pc.close(); this.pc = null; }
        this.videoStream.srcObject = null;
        this.updateStatus('Stopped', 'disconnected');
        this.videoControlBtns.start.disabled = false;
        this.videoControlBtns.stop.disabled = true;
        this.bytesReceivedEl.textContent = 'N/A';
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
        this.requestBtn.disabled = true;
        this.requestBtn.textContent = '⏳ Requesting...';
        this.log('Requesting access...', 'info');
    }


    releaseControl() {
        this.hasControl = false;
        this.position = 0;
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
        //console.log("getUserId: ", this.user_id);
        return this.user_id;
    }
    setUserId(id) {
        this.user_id = id;
        localStorage.setItem("user_id", id);
        //console.log("setUserId: ", id);
    }

    handleServerMessage(msg) {

        //console.log("server message: ", msg);

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
                if (this.position !== msg.position) {
                    this.log(`Your position in queue: ${msg.position}`, 'info');
                    this.position = msg.position;
                }
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