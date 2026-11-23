//! The string table of the login, account and session text (specs/DREAMFERENCE_AUTH.md §8.5).
//!
//! English, Portuguese, Spanish, German and French. The language comes from `LC_ALL`,
//! `LC_MESSAGES` and `LANG` in that order (the first one set and non-empty decides); a value that
//! names none of the five, or no value, gives English. Placeholders are `{0}`, `{1}`, … filled by
//! [`fmt`].

/// A language the table carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    Pt,
    Es,
    De,
    Fr,
}

impl Lang {
    /// Every language of the table.
    pub const ALL: [Lang; 5] = [Lang::En, Lang::Pt, Lang::Es, Lang::De, Lang::Fr];

    /// The language a locale value names (`pt_BR.UTF-8`, `de`, `fr_CA`), if it is one of the five.
    pub fn from_locale(value: &str) -> Option<Lang> {
        let code: String = value.chars().take_while(|c| c.is_ascii_alphabetic()).collect::<String>().to_ascii_lowercase();
        match code.as_str() {
            "en" => Some(Lang::En),
            "pt" => Some(Lang::Pt),
            "es" => Some(Lang::Es),
            "de" => Some(Lang::De),
            "fr" => Some(Lang::Fr),
            _ => None,
        }
    }

    /// The language of the environment: the first of `LC_ALL`, `LC_MESSAGES`, `LANG` that is set
    /// and not empty decides; English when it names none of the five.
    pub fn detect() -> Lang {
        Lang::from_vars(|name| std::env::var(name).ok())
    }

    /// [`Lang::detect`] over a lookup function, for tests.
    pub fn from_vars(get: impl Fn(&str) -> Option<String>) -> Lang {
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .filter_map(|name| get(name))
            .find(|value| !value.is_empty())
            .and_then(|value| Lang::from_locale(&value))
            .unwrap_or(Lang::En)
    }

    /// The language's two-letter code (`en`, `pt`, `es`, `de`, `fr`).
    pub fn code(self) -> &'static str {
        ["en", "pt", "es", "de", "fr"][self.index()]
    }

    fn index(self) -> usize {
        self as usize
    }
}

macro_rules! table {
    ($($key:ident => [$en:expr, $pt:expr, $es:expr, $de:expr, $fr:expr],)*) => {
        /// A string of the table.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Key { $($key),* }

        impl Key {
            /// Every key of the table.
            pub const ALL: &'static [Key] = &[$(Key::$key),*];

            fn row(self) -> [&'static str; 5] {
                match self { $(Key::$key => [$en, $pt, $es, $de, $fr]),* }
            }
        }
    };
}

table! {
    WrongCredentials => [
        "Wrong username or password.",
        "Nome de usuário ou senha incorretos.",
        "Usuario o contraseña incorrectos.",
        "Benutzername oder Passwort falsch.",
        "Nom d'utilisateur ou mot de passe incorrect."],
    NameTaken => [
        "That username is taken.",
        "Esse nome de usuário já está em uso.",
        "Ese nombre de usuario ya está en uso.",
        "Dieser Benutzername ist bereits vergeben.",
        "Ce nom d'utilisateur est déjà pris."],
    BadName => [
        "Usernames are 1-32 letters, digits, '.', '_' or '-'.",
        "Nomes de usuário têm de 1 a 32 letras, dígitos, '.', '_' ou '-'.",
        "Los nombres de usuario tienen de 1 a 32 letras, dígitos, '.', '_' o '-'.",
        "Benutzernamen bestehen aus 1-32 Buchstaben, Ziffern, '.', '_' oder '-'.",
        "Les noms d'utilisateur comptent de 1 à 32 lettres, chiffres, '.', '_' ou '-'."],
    PasswordMismatch => [
        "The passwords do not match.",
        "As senhas não coincidem.",
        "Las contraseñas no coinciden.",
        "Die Passwörter stimmen nicht überein.",
        "Les mots de passe ne correspondent pas."],
    PhonesPairAgain => [
        "Phones need to pair again.",
        "Os celulares precisam ser pareados novamente.",
        "Los teléfonos deben emparejarse de nuevo.",
        "Telefone müssen erneut gekoppelt werden.",
        "Les téléphones doivent être appairés à nouveau."],
    NoTokens => [
        "The accounts server sent no tokens.",
        "O servidor de contas não enviou tokens.",
        "El servidor de cuentas no envió tokens.",
        "Der Kontenserver hat keine Tokens gesendet.",
        "Le serveur de comptes n'a envoyé aucun jeton."],
    ServerAnswered => [
        "The accounts server answered {0}.",
        "O servidor de contas respondeu {0}.",
        "El servidor de cuentas respondió {0}.",
        "Der Kontenserver antwortete {0}.",
        "Le serveur de comptes a répondu {0}."],
    FieldUsername => ["Username", "Nome de usuário", "Usuario", "Benutzername", "Nom d'utilisateur"],
    FieldPassword => ["Password", "Senha", "Contraseña", "Passwort", "Mot de passe"],
    FieldConfirmPassword => [
        "Confirm password", "Confirmar senha", "Confirmar contraseña", "Passwort bestätigen", "Confirmer le mot de passe"],
    FieldNewPassword => [
        "New password", "Nova senha", "Nueva contraseña", "Neues Passwort", "Nouveau mot de passe"],
    FieldConfirm => ["Confirm", "Confirmar", "Confirmar", "Bestätigen", "Confirmer"],
    SignInTitle => [
        "Sign in to {0}", "Entrar em {0}", "Iniciar sesión en {0}", "Bei {0} anmelden", "Se connecter à {0}"],
    SignInHint => [
        "With the username and password of your account",
        "Com o nome de usuário e a senha da sua conta",
        "Con el usuario y la contraseña de su cuenta",
        "Mit Benutzername und Passwort Ihres Kontos",
        "Avec le nom d'utilisateur et le mot de passe de votre compte"],
    CreateTitle => [
        "Create an account on {0}", "Criar uma conta em {0}", "Crear una cuenta en {0}",
        "Konto bei {0} erstellen", "Créer un compte sur {0}"],
    CreateHint => [
        "Choose a username and a password",
        "Escolha um nome de usuário e uma senha",
        "Elija un usuario y una contraseña",
        "Wählen Sie einen Benutzernamen und ein Passwort",
        "Choisissez un nom d'utilisateur et un mot de passe"],
    DeviceTitle => [
        "Sign in with a device code", "Entrar com um código de dispositivo",
        "Iniciar sesión con un código de dispositivo", "Mit einem Gerätecode anmelden",
        "Se connecter avec un code d'appareil"],
    DeviceHint => [
        "Sign in from another device with a one-time code",
        "Entre a partir de outro dispositivo com um código de uso único",
        "Inicie sesión desde otro dispositivo con un código de un solo uso",
        "Von einem anderen Gerät mit einem Einmalcode anmelden",
        "Connectez-vous depuis un autre appareil avec un code à usage unique"],
    ApiKeyTitle => [
        "Provide your own API key", "Informe sua própria chave de API",
        "Proporcione su propia clave de API", "Eigenen API-Schlüssel angeben", "Fournir votre propre clé d'API"],
    ApiKeyHint => [
        "Paste an API key minted with /apikey or by an admin",
        "Cole uma chave de API criada com /apikey ou por um administrador",
        "Pegue una clave de API creada con /apikey o por un administrador",
        "Fügen Sie einen mit /apikey oder von einem Admin erstellten API-Schlüssel ein",
        "Collez une clé d'API créée avec /apikey ou par un administrateur"],
    SignedIn => [
        "Signed in to {0} as {1}", "Conectado a {0} como {1}", "Sesión iniciada en {0} como {1}",
        "Bei {0} als {1} angemeldet", "Connecté à {0} en tant que {1}"],
    SignedInOnly => [
        "Signed in to {0}", "Conectado a {0}", "Sesión iniciada en {0}", "Bei {0} angemeldet", "Connecté à {0}"],
    FirstUserAdmin => [
        "You are the first user of {0} and its admin. Others can create accounts at the login screen; /users manages them.",
        "Você é o primeiro usuário de {0} e seu administrador. Outros podem criar contas na tela de login; /users as gerencia.",
        "Usted es el primer usuario de {0} y su administrador. Otros pueden crear cuentas en la pantalla de inicio de sesión; /users las gestiona.",
        "Sie sind der erste Benutzer von {0} und dessen Admin. Andere können auf dem Anmeldebildschirm Konten erstellen; /users verwaltet sie.",
        "Vous êtes le premier utilisateur de {0} et son administrateur. Les autres peuvent créer des comptes sur l'écran de connexion ; /users les gère."],
    SigningIn => ["Signing in…", "Entrando…", "Iniciando sesión…", "Anmeldung läuft…", "Connexion…"],
    PressEnterTo => ["Press Enter {0}", "Pressione Enter {0}", "Pulse Enter {0}", "Enter drücken: {0}", "Appuyez sur Entrée {0}"],
    ToContinue => ["to continue", "para continuar", "para continuar", "um fortzufahren", "pour continuer"],
    RetryOrBack => [
        "Press r to try again, Esc to go back",
        "Pressione r para tentar de novo, Esc para voltar",
        "Pulse r para reintentar, Esc para volver",
        "r drücken für einen neuen Versuch, Esc für zurück",
        "Appuyez sur r pour réessayer, Échap pour revenir"],
    FormKeys => [
        "Press Enter to continue, Tab for the next field, Esc to go back",
        "Pressione Enter para continuar, Tab para o próximo campo, Esc para voltar",
        "Pulse Enter para continuar, Tab para el siguiente campo, Esc para volver",
        "Enter zum Fortfahren, Tab für das nächste Feld, Esc für zurück",
        "Appuyez sur Entrée pour continuer, Tab pour le champ suivant, Échap pour revenir"],
    PlainHttp => [
        "{0} is reached over plain HTTP; the password is not sent.",
        "{0} é acessado por HTTP simples; a senha não será enviada.",
        "{0} se alcanza por HTTP sin cifrar; la contraseña no se enviará.",
        "{0} ist nur über unverschlüsseltes HTTP erreichbar; das Passwort wird nicht gesendet.",
        "{0} est joint en HTTP simple ; le mot de passe ne sera pas envoyé."],
    CouldNotStart => [
        "Could not start the sign-in.", "Não foi possível iniciar o login.", "No se pudo iniciar el inicio de sesión.",
        "Die Anmeldung konnte nicht gestartet werden.", "Impossible de démarrer la connexion."],
    CertUnreadable => [
        "Could not read the node's certificate.", "Não foi possível ler o certificado do nó.",
        "No se pudo leer el certificado del nodo.", "Das Zertifikat des Knotens konnte nicht gelesen werden.",
        "Impossible de lire le certificat du nœud."],
    CouldNotSave => [
        "Could not save the sign-in: {0}", "Não foi possível salvar o login: {0}",
        "No se pudo guardar el inicio de sesión: {0}", "Die Anmeldung konnte nicht gespeichert werden: {0}",
        "Impossible d'enregistrer la connexion : {0}"],
    SignedInAs => [
        "Signed in as {0} ({1})", "Conectado como {0} ({1})", "Sesión iniciada como {0} ({1})",
        "Angemeldet als {0} ({1})", "Connecté en tant que {0} ({1})"],
    SignedInAsAt => [
        "Signed in as {0} ({1}) at {2}", "Conectado como {0} ({1}) em {2}", "Sesión iniciada como {0} ({1}) en {2}",
        "Angemeldet als {0} ({1}) bei {2}", "Connecté en tant que {0} ({1}) sur {2}"],
    UsingApiKey => [
        "Using API key (the gate names its user on the first request).",
        "Usando chave de API (o portão informa o usuário na primeira requisição).",
        "Usando clave de API (la puerta indica su usuario en la primera solicitud).",
        "API-Schlüssel in Gebrauch (das Gate nennt den Benutzer bei der ersten Anfrage).",
        "Clé d'API utilisée (la passerelle nomme son utilisateur à la première requête)."],
    NotSignedIn => [
        "Not signed in. Run ragon login.", "Não conectado. Execute ragon login.",
        "Sin sesión iniciada. Ejecute ragon login.", "Nicht angemeldet. Führen Sie ragon login aus.",
        "Non connecté. Lancez ragon login."],
    Usage => ["Usage: {0}", "Uso: {0}", "Uso: {0}", "Verwendung: {0}", "Utilisation : {0}"],
    SessionEnded => [
        "Not signed in, or the session ended. Run ragon login.",
        "Não conectado, ou a sessão terminou. Execute ragon login.",
        "Sin sesión iniciada, o la sesión terminó. Ejecute ragon login.",
        "Nicht angemeldet, oder die Sitzung ist beendet. Führen Sie ragon login aus.",
        "Non connecté, ou la session est terminée. Lancez ragon login."],
    NeedsAdmin => [
        "/users needs the admin role.", "/users exige o papel de administrador.",
        "/users requiere el rol de administrador.", "/users erfordert die Admin-Rolle.",
        "/users nécessite le rôle d'administrateur."],
    NodeAnswered => [
        "the node answered {0}", "o nó respondeu {0}", "el nodo respondió {0}", "der Knoten antwortete {0}",
        "le nœud a répondu {0}"],
    NothingToShow => [
        "Nothing to show.", "Nada para mostrar.", "Nada que mostrar.", "Nichts anzuzeigen.", "Rien à afficher."],
    NewApiKey => [
        "New API key (shown once): {0}", "Nova chave de API (mostrada uma vez): {0}",
        "Nueva clave de API (se muestra una vez): {0}", "Neuer API-Schlüssel (wird einmal angezeigt): {0}",
        "Nouvelle clé d'API (affichée une seule fois) : {0}"],
    Done => ["Done.", "Pronto.", "Hecho.", "Fertig.", "Terminé."],
    FirstAccount => [
        "{0} has no accounts yet. Create the first one: it is the admin of {0}.",
        "{0} ainda não tem contas. Crie a primeira: ela é a administradora de {0}.",
        "{0} aún no tiene cuentas. Cree la primera: será la administradora de {0}.",
        "{0} hat noch keine Konten. Erstellen Sie das erste: es ist der Admin von {0}.",
        "{0} n'a pas encore de compte. Créez le premier : il est l'administrateur de {0}."],
    FirstAccountElsewhere => [
        "{0} has no accounts yet. Its first account is made on {0} itself: run ragon in a terminal there, or ragon-admin node user add <name>. Running without sign-in until then.",
        "{0} ainda não tem contas. A primeira conta é criada no próprio {0}: execute ragon em um terminal lá, ou ragon-admin node user add <nome>. Até lá, sem login.",
        "{0} aún no tiene cuentas. La primera cuenta se crea en el propio {0}: ejecute ragon en una terminal allí, o ragon-admin node user add <nombre>. Hasta entonces, sin inicio de sesión.",
        "{0} hat noch keine Konten. Das erste Konto wird auf {0} selbst erstellt: Starten Sie dort ragon in einem Terminal oder ragon-admin node user add <Name>. Bis dahin ohne Anmeldung.",
        "{0} n'a pas encore de compte. Le premier compte se crée sur {0} même : lancez ragon dans un terminal là-bas, ou ragon-admin node user add <nom>. D'ici là, sans connexion."],
    NoAccounts => [
        "This node has no accounts.", "Este nó não tem contas.", "Este nodo no tiene cuentas.",
        "Dieser Knoten hat keine Konten.", "Ce nœud n'a aucun compte."],
    CreateFirstTitle => [
        "Create the first account on {0}", "Criar a primeira conta em {0}", "Crear la primera cuenta en {0}",
        "Das erste Konto auf {0} anlegen", "Créer le premier compte sur {0}"],
    CreateFirstHint => [
        "The first account is the admin of {0}. Choose a username and a password.",
        "A primeira conta é a administradora de {0}. Escolha um nome de usuário e uma senha.",
        "La primera cuenta es la administradora de {0}. Elija un nombre de usuario y una contraseña.",
        "Das erste Konto ist der Admin von {0}. Wählen Sie einen Benutzernamen und ein Passwort.",
        "Le premier compte est l'administrateur de {0}. Choisissez un nom d'utilisateur et un mot de passe."],
    FirstAccountOnNode => [
        "{0} has no accounts yet. Its first account is made on {0} itself: run ragon in a terminal there.",
        "{0} ainda não tem contas. A primeira conta é criada no próprio {0}: execute ragon num terminal lá.",
        "{0} aún no tiene cuentas. La primera cuenta se crea en el propio {0}: ejecute ragon en una terminal allí.",
        "{0} hat noch keine Konten. Das erste Konto wird auf {0} selbst angelegt: starten Sie dort ragon in einem Terminal.",
        "{0} n'a encore aucun compte. Le premier compte se crée sur {0} même : lancez ragon dans un terminal là-bas."],
    EmptyPasswordAsk => [
        "Continue without a password?", "Continuar sem senha?", "¿Continuar sin contraseña?",
        "Ohne Passwort fortfahren?", "Continuer sans mot de passe ?"],
    AccountsUnreachable => [
        "The accounts server cannot be reached.", "Não foi possível contactar o servidor de contas.",
        "No se puede contactar con el servidor de cuentas.", "Der Kontenserver ist nicht erreichbar.",
        "Le serveur de comptes est injoignable."],
    AccountLabel => ["Account", "Conta", "Cuenta", "Konto", "Compte"],
    SignInAction => ["Sign in", "Entrar", "Iniciar sesión", "Anmelden", "Se connecter"],
    CreateAction => ["Create account", "Criar conta", "Crear cuenta", "Konto anlegen", "Créer le compte"],
    CreateInstead => [
        "Create an account", "Criar uma conta", "Crear una cuenta", "Ein Konto anlegen", "Créer un compte"],
    SignInInstead => [
        "I already have an account", "Já tenho uma conta", "Ya tengo una cuenta", "Ich habe schon ein Konto",
        "J'ai déjà un compte"],
    SignOutAction => ["Sign out", "Sair", "Cerrar sesión", "Abmelden", "Se déconnecter"],
    ContinueAction => ["Continue", "Continuar", "Continuar", "Fortfahren", "Continuer"],
    BackAction => ["Back", "Voltar", "Atrás", "Zurück", "Retour"],
    NotNowAction => ["Not now", "Agora não", "Ahora no", "Nicht jetzt", "Pas maintenant"],
    RetryAction => ["Try again", "Tentar novamente", "Reintentar", "Erneut versuchen", "Réessayer"],
}

/// The text of `key` in `lang`, placeholders unfilled.
pub fn text(lang: Lang, key: Key) -> &'static str {
    key.row()[lang.index()]
}

/// The text of `key` in `lang` with `{0}`, `{1}`, … replaced by `args`.
pub fn fmt_in(lang: Lang, key: Key, args: &[&str]) -> String {
    let mut out = text(lang, key).to_string();
    for (i, arg) in args.iter().enumerate() {
        out = out.replace(&format!("{{{i}}}"), arg);
    }
    out
}

/// The text of `key` in the environment's language.
pub fn t(key: Key) -> &'static str {
    text(Lang::detect(), key)
}

/// [`fmt_in`] in the environment's language.
pub fn fmt(key: Key, args: &[&str]) -> String {
    fmt_in(Lang::detect(), key, args)
}
