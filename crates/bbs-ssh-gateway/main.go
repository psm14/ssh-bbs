package main

import (
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"fmt"
	"io"
	"log"
	"net"
	"os"
	"os/exec"
	"strings"
	"time"

	pty "github.com/creack/pty"
	glssh "github.com/gliderlabs/ssh"
	gossh "golang.org/x/crypto/ssh"
)

func main() {
	addr := ":2222"
	clientPath := getenv("BBS_CLIENT_PATH", "/app/bbs-tui")
	defaultRoom := getenv("BBS_DEFAULT_ROOM", "lobby")
	databaseURL := os.Getenv("DATABASE_URL")

	signer := mustHostKey()

	glssh.Handle(func(s glssh.Session) {
		// Require PTY
		ptyReq, winCh, ok := s.Pty()
		if !ok {
			io.WriteString(s, "A PTY is required.\n")
			_ = s.Exit(1)
			return
		}

		pk := s.PublicKey()
		fp := "unknown"
		ktype := "unknown"
		if pk != nil {
			fp = gossh.FingerprintSHA256(pk)
			ktype = mapKeyType(pk.Type())
		}
		log.Printf("connect remote=%s key=%s fp=%s", remoteAddr(s), ktype, shortFP(fp))

		// Prepare command
		cmd := exec.CommandContext(s.Context(), clientPath)
		cmd.Env = append(os.Environ(),
			"BBS_PUBKEY_SHA256="+fp,
			"BBS_PUBKEY_TYPE="+ktype,
			"REMOTE_ADDR="+remoteAddr(s),
			"DATABASE_URL="+databaseURL,
			"BBS_DEFAULT_ROOM="+defaultRoom,
		)

		// Allocate PTY for the child
		f, err := pty.Start(cmd)
		if err != nil {
			fmt.Fprintf(s, "failed to start client: %v\n", err)
			_ = s.Exit(1)
			return
		}
		defer f.Close()

		// Set initial window size
		_ = pty.Setsize(f, &pty.Winsize{Cols: uint16(ptyReq.Window.Width), Rows: uint16(ptyReq.Window.Height)})

		// Propagate future window changes
		go func() {
			for w := range winCh {
				_ = pty.Setsize(f, &pty.Winsize{Cols: uint16(w.Width), Rows: uint16(w.Height)})
			}
		}()

		// Pipe data
		go func() { _, _ = io.Copy(f, s) }()
		_, _ = io.Copy(s, f)

		_ = cmd.Wait()
		log.Printf("disconnect remote=%s", remoteAddr(s))
	})

	// Public key auth: allow modern algorithms only
	server := &glssh.Server{
		Addr:        addr,
		Handler:     glssh.Handler(func(s glssh.Session) { /* replaced by global Handle above, keep for clarity */ }),
		Version:     "SSH-2.0-bbs-ssh-gateway",
		IdleTimeout: 2 * time.Hour,
		PublicKeyHandler: func(ctx glssh.Context, key glssh.PublicKey) bool {
			t := key.Type()
			allowed := map[string]bool{
				"ssh-ed25519":                true,
				"ecdsa-sha2-nistp256":        true,
				"ecdsa-sha2-nistp384":        true,
				"rsa-sha2-256":               true,
				"rsa-sha2-512":               true,
				"sk-ssh-ed25519@openssh.com": true,
			}
			if !allowed[t] {
				return false
			}
			return true
		},
		PasswordHandler:               func(ctx glssh.Context, pass string) bool { return false },
		LocalPortForwardingCallback:   func(ctx glssh.Context, dhost string, dport uint32) bool { return false },
		ReversePortForwardingCallback: func(ctx glssh.Context, host string, port uint32) bool { return false },
	}
	server.AddHostKey(signer)

	log.Printf("listening on %s; client=%s room=%s", addr, clientPath, defaultRoom)
	if err := server.ListenAndServe(); err != nil {
		log.Fatalf("ssh server error: %v", err)
	}
}

func mustHostKey() gossh.Signer {
	// Generate an ephemeral ed25519 host key each boot
	// (compose can add a volume and persist later)
	_, priv, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		log.Fatalf("hostkey gen error: %v", err)
	}
	signer, err := gossh.NewSignerFromKey(priv)
	if err != nil {
		log.Fatalf("hostkey signer error: %v", err)
	}
	return signer
}

func getenv(k, def string) string {
	if v := os.Getenv(k); v != "" {
		return v
	}
	return def
}

func mapKeyType(t string) string {
	switch t {
	case "ssh-ed25519":
		return "ed25519"
	case "ecdsa-sha2-nistp256":
		return "ecdsa256"
	case "ecdsa-sha2-nistp384":
		return "ecdsa384"
	case "rsa-sha2-256":
		return "rsa256"
	case "rsa-sha2-512":
		return "rsa512"
	case "sk-ssh-ed25519@openssh.com":
		return "sk-ed25519"
	default:
		return t
	}
}

func remoteAddr(s glssh.Session) string {
	ra := s.RemoteAddr()
	if ra == nil {
		return ""
	}
	// normalize to host:port, without zone
	host, port, err := net.SplitHostPort(ra.String())
	if err != nil {
		return ra.String()
	}
	if i := strings.IndexByte(host, '%'); i >= 0 {
		host = host[:i]
	}
	return net.JoinHostPort(host, port)
}

// Ensure the command is terminated if the session context is cancelled.
func killOnDone(ctx context.Context, cmd *exec.Cmd) {
	go func() {
		<-ctx.Done()
		_ = cmd.Process.Kill()
	}()
}

func shortFP(fp string) string {
	if strings.HasPrefix(fp, "SHA256:") {
		fp = strings.TrimPrefix(fp, "SHA256:")
	}
	if len(fp) > 8 {
		return fp[:8]
	}
	return fp
}
