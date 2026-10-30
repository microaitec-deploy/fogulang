
import {Fogu} from "./src/fogu";
     const client = Fogu();
     try {
            const url=process.env.FOGU_URL||''
            await client.connect(url);
        //   client.kvdb = await client.channel("xkolanon.etcd");
            client.kv= await client.channel('xkolanon.kv');
        // await client.kvdb.join("xkolanon.etcd");
            await client.kv.join("xkolanon.kv");
     } catch (error) {
        console.log(error)
     }




    try {

const input = {
  key: "web.home",
  bucket: 'website',
  value: JSON.stringify({
  background: {
    video: "/videos/0020.mp4",
    type: "video/mp4"
  },
 slides:[

{
  id: 1,
  title: "Jardim de Infância",
  description: `
    <div className="text-center mb-5">
      <div className="inline-flex items-center gap-2 px-4 py-2 bg-gradient-to-r from-emerald-500/20 to-green-500/20 border border-emerald-400/40 rounded-full mb-4">
        <span className="relative flex h-2 w-2">
          <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75"></span>
          <span className="relative inline-flex rounded-full h-2 w-2 bg-emerald-500"></span>
        </span>
        <span className="text-emerald-300 text-xs font-bold uppercase tracking-widest">
          BREVE DISPONIBILIDADE
        </span>
      </div>

      <h3 className="text-white font-bold text-2xl mb-2">
        Brincar, Aprender e Crescer
      </h3>

      <p className="text-gray-400 text-sm max-w-md mx-auto">
        Um espaço educativo pensado para tornar a infância mais feliz,
        criativa e enriquecedora. A Xkola Nón apoia crianças, educadores
        e encarregados de educação através de brincadeiras educativas,
        jogos, desenhos, músicas e atividades que estimulam a imaginação,
        a curiosidade, a autonomia e a convivência na escola.
      </p>
    </div>

    <div className="my-4 grid grid-cols-2 gap-3">
      <div className="p-3 bg-pink-500/10 rounded-lg border border-pink-500/30 text-center">
        <p className="text-white text-sm font-semibold">Brincadeiras</p>
        <p className="text-gray-400 text-xs mt-1">
          Jogos e diversão em grupo
        </p>
      </div>

      <div className="p-3 bg-rose-500/10 rounded-lg border border-rose-500/30 text-center">
        <p className="text-white text-sm font-semibold">Criatividade</p>
        <p className="text-gray-400 text-xs mt-1">
          Desenhos, música e imaginação
        </p>
      </div>

      <div className="p-3 bg-fuchsia-500/10 rounded-lg border border-fuchsia-500/30 text-center">
        <p className="text-white text-sm font-semibold">Aprendizagem</p>
        <p className="text-gray-400 text-xs mt-1">
          Atividades lúdicas e educativas
        </p>
      </div>

      <div className="p-3 bg-purple-500/10 rounded-lg border border-purple-500/30 text-center">
        <p className="text-white text-sm font-semibold">Desenvolvimento</p>
        <p className="text-gray-400 text-xs mt-1">
          Socialização e autonomia
        </p>
      </div>
    </div>

    <div className="mt-5 p-4 bg-gradient-to-r from-emerald-500/20 to-green-500/20 rounded-xl border-2 border-emerald-500/40 text-center">
      <p className="text-white font-bold text-base mb-2">
        Uma Infância Feliz Começa a Brincar
      </p>
      <p className="text-gray-300 text-sm">
        Uma plataforma educativa moderna que aproxima crianças,
        educadores e famílias, acompanhando a aprendizagem e o
        desenvolvimento infantil de forma simples, divertida e inovadora.
      </p>
    </div>
  `,
  imageSrc: "/images/jardim_infancia.png",
  imageAlt: "Crianças santomenses a brincar, aprender e construir com blocos coloridos na escola.",
  imageLabel: "Jardim de Infância",
  features: [
    "Jogos Educativos",
    "Atividades Criativas",
    "Fotos e Vídeos",
    "Acompanhamento Infantil"
  ],
  stats: [
    { value: "0-5", label: "Anos" },
    { value: "100%", label: "Dedicação" },
    { value: "360°", label: "Desenvolvimento" }
  ],
  lead: {
    enabled: true,
    cta: "Conhecer o Jardim de Infância",
    fields: ["nome", "email", "telefone"],
    destination: "https://xkolanon.com/institutions/online",
    tag: "demo_jardim"
  }
},

{
  id: 1,
  title: "Jardim de Infância",
  description: `
    <div className="text-center mb-5">
      <div className="inline-flex items-center gap-2 px-4 py-2 bg-gradient-to-r from-emerald-500/20 to-green-500/20 border border-emerald-400/40 rounded-full mb-4">
        <span className="relative flex h-2 w-2">
          <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75"></span>
          <span className="relative inline-flex rounded-full h-2 w-2 bg-emerald-500"></span>
        </span>
        <span className="text-emerald-300 text-xs font-bold uppercase tracking-widest">
          CANDIDATURAS ABERTAS
        </span>
      </div>

      <h3 className="text-white font-bold text-2xl mb-2">
        Candidate a sua escola
      </h3>

      <p className="text-gray-400 text-sm max-w-md mx-auto">
        Faça parte da Xkola Nón e prepare o seu jardim de infância
        para uma nova forma de gerir, acompanhar e valorizar a
        educação infantil. Uma plataforma digital que aproxima
        instituições de ensino, educadores e encarregados de educação,
        reunindo num só lugar a gestão escolar e o acompanhamento
        diário das crianças.
      </p>
    </div>

    <div className="my-4 grid grid-cols-2 gap-3">
      <div className="p-3 bg-pink-500/10 rounded-lg border border-pink-500/30 text-center">
        <p className="text-white text-sm font-semibold">Gestão Escolar</p>
        <p className="text-gray-400 text-xs mt-1">
          Informação organizada num só lugar
        </p>
      </div>

      <div className="p-3 bg-rose-500/10 rounded-lg border border-rose-500/30 text-center">
        <p className="text-white text-sm font-semibold">Comunicação</p>
        <p className="text-gray-400 text-xs mt-1">
          Mais proximidade com as famílias
        </p>
      </div>

      <div className="p-3 bg-fuchsia-500/10 rounded-lg border border-fuchsia-500/30 text-center">
        <p className="text-white text-sm font-semibold">Acompanhamento</p>
        <p className="text-gray-400 text-xs mt-1">
          Rotinas e actividades das crianças
        </p>
      </div>

      <div className="p-3 bg-purple-500/10 rounded-lg border border-purple-500/30 text-center">
        <p className="text-white text-sm font-semibold">Inovação</p>
        <p className="text-gray-400 text-xs mt-1">
          Uma experiência educativa mais moderna
        </p>
      </div>
    </div>

    <div className="mt-5 p-4 bg-gradient-to-r from-emerald-500/20 to-green-500/20 rounded-xl border-2 border-emerald-500/40 text-center">
      <p className="text-white font-bold text-base mb-2">
        Candidata a sua escola
      </p>
      <p className="text-gray-300 text-sm">
        Dê o primeiro passo para integrar a sua instituição na
        Xkola Nón. Apresente a sua escola e manifeste o seu interesse
        em fazer parte de uma plataforma criada para simplificar
        a gestão escolar e aproximar toda a comunidade educativa.
      </p>
    </div>
  `,
  imageSrc: "/images/image_no_background.png",
  imageAlt: "Crianças a brincar, aprender e construir com blocos coloridos num jardim de infância.",
  imageLabel: "Candidate o seu Jardim de Infância",
  features: [
    "Gestão Escolar",
    "Comunicação com Famílias",
    "Acompanhamento Infantil",
    "Actividades Educativas"
  ],
  stats: [
    { value: "0-5", label: "Anos" },
    { value: "1", label: "Plataforma" },
    { value: "360°", label: "Acompanhamento" }
  ],
  lead: {
    enabled: true,
    cta: "Candidatar a minha escola",
    fields: ["nome", "email", "telefone"],
    destination: "https://xkolanon.com/institutions/online",
    tag: "candidatura_jardim"
  }
}
]
})
};


const res = await client.kv.call("kv.set",input);
console.log("resultado:", res); // { result: 5 }
} catch (error) {
  
}



try {
    

const input = {
  bucket: 'website',
  key: "web.pricing",
  value: JSON.stringify({
     planCategories: [
{
  "id": "schools",
  "name": "Escolas",
  "icon": "Building",
  "description": "Creches, Básicas, Secundárias e Técnico-Profissionais",
  "tiers": [
    {
      "popular": false,
      "name": "Bikin",
      "display_name": "Essencial",
      "period": "por ano",
      "price": 14112,
      "currency": "STN",
      "description": "Gestão escolar e presença digital num só lugar. O plano ideal para pequenas escolas que querem dar o próximo passo.",
      "monthly_equivalent": 1176,
      "limits": {
        "users": {
          "directors": 1,
          "subdirectors": 1,
          "coordinators": 1,
          "secretaries": 1,
          "educators": 10,
          "students": 150,
          "drivers": 3,
          "vehicles": 3,
          "administrative": 6
        },
        "classes": 5,
        "courses": 3,
        "storage": "1 GB",
        "publications": {
          "announcements": 5,
          "vacancies": 3
        },
        "reports": {
          "monthly": true,
          "custom": false,
          "export": "PDF"
        },
        "email": {
          "monthly_limit": 500,
          "bulk_sending": false,
          "templates": 3
        },
        "historical_retention": "5 anos",
        "website_included": true
      },
      "features": [
        "Página Web Institucional Integrada (Gerida pela Xkolanón)",
        "Até 150 Alunos Ativos | 5 Turmas | 3 Cursos",
        "Arquivo Digital com Persistência Histórica (Mínimo 5 anos)",
        "Gestão de Candidaturas e Vagas (3 por nível/ano)",
        "Matrículas, Processos Individuais e Documentos dos Alunos",
        "Livro de Ponto e Assiduidade Digital",
        "Comunicação Escolar (Notícias, Avisos, Eventos e Comunicados)",
        "Gestão de Transportes e Motoristas (Até 3)",
        "6 Utilizadores Administrativos com Permissões Configuráveis",
        "Acesso via Internet com Segurança, Backups e Atualizações",
        "Suporte por Email"
      ],
      "cta": "Garantir Presença Digital",
      "badge": "Arquivo 5 Anos",
      "color": "from-slate-500 to-gray-500",
      "popular": false
    },
    {
      "popular": true,
      "name": "Fôrtê",
      "period": "por ano",
      "price": 29500,
      "currency": "STN",
      "description": "Gestão pedagógica completa com funcionalidades avançadas.",
      "limits": {
        "users": {
          "directors": 2,
          "subdirectors": 2,
          "coordinators": 5,
          "secretaries": 3,
          "educators": 20,
          "students": 300,
          "drivers": 5,
          "vehicles": 6
        },
        "classes": 999,
        "storage": "20 GB",
        "publications": {
          "announcements": 50,
          "vacancies": 10
        },
        "reports": {
          "monthly": true,
          "custom": true,
          "export": ["PDF", "Excel", "CSV"]
        },
        "email": {
          "monthly_limit": 2000,
          "bulk_sending": true,
          "templates": 10
        },
        "website_included": true
      },
      "features": [
        "Até 300 alunos | Turmas ilimitadas",
        "Arquivo Digital Legal (Mínimo 5 anos)",
        "Tudo do plano Bikin (Essencial)",
        "Planos de Aula Personalizáveis (Currículo STP)",
        "Relatórios de Avaliação Automatizados",
        "Avaliações, Notas e Pautas Digitais",
        "Emissão de Certificados e Declarações",
        "Horários Escolares",
        "Dashboards de Turmas",
        "Suporte Prioritário via WhatsApp"
      ],
      "cta": "Aderir Plano Fôrtê",
      "badge": "Mais Popular",
      "color": "from-blue-500 to-cyan-500"
    },
    {
      "popular": false,
      "name": "Grandi",
      "period": "por ano",
      "price": 49000,
      "currency": "STN",
      "description": "Gestão institucional completa com auditoria e conformidade total.",
      "limits": {
        "users": {
          "directors": 5,
          "subdirectors": 5,
          "coordinators": 999,
          "secretaries": 10,
          "educators": 999,
          "students": 9999,
          "drivers": 20,
          "vehicles": 30
        },
        "classes": 999,
        "storage": "100 GB",
        "publications": {
          "announcements": 999,
          "vacancies": 999
        },
        "reports": {
          "monthly": true,
          "custom": true,
          "export": ["PDF", "Excel", "CSV", "JSON"]
        },
        "email": {
          "monthly_limit": 10000,
          "bulk_sending": true,
          "templates": 50
        },
        "website_included": true
      },
      "features": [
        "Alunos Ilimitados",
        "Arquivo Digital Vitalício (Enquanto ativo)",
        "Tudo do plano Fôrtê",
        "Gestão Financeira e Recibos Legais (Propinas)",
        "Histórico Escolar Consolidado",
        "Dashboard de Auditoria para Direção",
        "Gestão Multi-Turma e Multi-Nível",
        "Formação Presencial do Secretariado",
        "Gestor de Conta Dedicado"
      ],
      "cta": "Contactar Consultor",
      "badge": "Enterprise",
      "color": "from-emerald-500 to-teal-500"
    }
  ]
}
  ],

  allFeatures: [
    {
      category: "Gestão Académica",
      items: [
        { name: "Gestão de Alunos", description: "Perfis detalhados com informações pessoais, académicas e processos individuais centralizados." },
        { name: "Registo de Assiduidade", description: "Livro de ponto digital com registo eficiente de entradas, saídas e justificações." },
        { name: "Gestão de Turmas", description: "Organização de turmas, horários e atribuição de professores por nível." },
        { name: "Avaliações e Exames", description: "Gestão de provas, exames, pautas digitais e registo automático de notas." },
        { name: "Gestão de Matrículas", description: "Processo digital de inscrição e matrícula com validação de documentos." },
        { name: "Arquivo Legal Escolar", description: "Guarda segura de dados académicos por mínimo de 5 anos, conforme normas legais." },
      ],
      icon: "GraduationCap",
      color: "from-blue-500 to-cyan-500",
    },
    {
      category: "Comunicação",
      items: [
        { name: "Portal do Encarregado", description: "Acesso dedicado para encarregados acompanharem o progresso e rotina diária." },
        { name: "Notificações Automáticas", description: "Alertas e comunicações automáticas para todas as partes envolvidas." },
        { name: "XKola Chat", description: "Sistema de mensagens integrado para comunicação em tempo real escola-família." },
        { name: "Agenda Escolar", description: "Calendário partilhado de eventos, atividades e publicações da escola." },
        { name: "Rotina Diária", description: "Registo e partilha da rotina diária da criança com os encarregados." },
      ],
      icon: "Users",
      color: "from-emerald-500 to-teal-500",
    },
    {
      category: "Administração",
      items: [
        { name: "Gestão Financeira", description: "Controlo de propinas, pagamentos, recibos legais e relatórios financeiros." },
        { name: "Biblioteca Digital", description: "Gestão de livros, materiais pedagógicos e empréstimos escolares." },
        { name: "Transporte Escolar", description: "Controlo de rotas, viaturas, motoristas e registo de segurança dos alunos." },
        { name: "Alimentação Escolar", description: "Gestão de refeições e controlo nutricional dos alunos." },
        { name: "Emissão de Documentos", description: "Geração de certificados, declarações e diplomas com validade legal." },
      ],
      icon: "Building",
      color: "from-violet-500 to-purple-500",
    },
    {
      category: "Inovação",
      items: [
        { name: "Planeamento Pedagógico", description: "Ferramentas avançadas para criação de planos de aula alinhados com o currículo nacional." },
        { name: "Ensino Virtual", description: "Plataforma integrada para aulas online, videoconferências e materiais interativos." },
        { name: "Analytics Avançado", description: "Relatórios inteligentes, dashboards de auditoria e insights preditivos." },
        { name: "Integração API", description: "Conexão com outras plataformas, sistemas do Ministério e serviços externos." },
      ],
      icon: "Sparkles",
      color: "from-amber-500 to-orange-500",
    },
  ]
  })

 
};

const res = await client.kv.call("kv.set",input);
console.log("resultado:", res); // { result: 5 }
} catch (error) {
  
}




try {
    

const input = {
  bucket: 'app',
  key: "levels",
  value: JSON.stringify([
        {
            id: "kindergarten",
            code: "kindergarten",
            title: "Jardim de Infância",
            subtitle: "Aceder ao Jardim de Infância",
            description: "Creche e Pré-Escolar. Educação infantil com carinho e dedicação para os mais pequenos.",
            icon: "PiBabyDuotone",
            iconBg: "bg-gradient-to-br from-pink-400 via-rose-400 to-pink-500",
            iconColor: "text-white",
            gradient: "from-pink-500/10 via-rose-400/5 to-amber-200/10",
            border: "border-pink-400/40",
            buttonBg: "bg-gradient-to-r from-pink-500 to-rose-500",
            buttonHover: "hover:from-pink-600 hover:to-rose-600",
            accentColor: "text-pink-400",
            overlayColor: "#FFE4E6",
            levels: ["Creche", "Pré-Escolar"],
            backgroundImage: "/images/jardim_infancia.png",
            disciplines: [],
            badge: "PRIMEIROS PASSOS",
            badgeColor: "from-pink-500/20 to-rose-500/20 border-pink-400/40 text-pink-300",
            available: true,
        },
        {
            id: "base_c1",
            code: "base_c1",
            title: "1.ª a 4.ª Classes",
            subtitle: "Aceder ao 1.º Ciclo do Básico",
            description: "Ensino Básico - 1.º Ciclo. Fundamentos da aprendizagem com 5 disciplinas essenciais.",
            icon: "TbBooks",
            iconBg: "bg-gradient-to-br from-blue-500 via-blue-600 to-indigo-600",
            iconColor: "text-white",
            gradient: "from-blue-500/10 via-indigo-500/5 to-cyan-400/10",
            border: "border-blue-400/40",
            buttonBg: "bg-gradient-to-r from-blue-500 to-indigo-600",
            buttonHover: "hover:from-blue-600 hover:to-indigo-700",
            accentColor: "text-blue-400",
            overlayColor: "#DBEAFE",
            levels: ["1.ª Classe", "2.ª Classe", "3.ª Classe", "4.ª Classe"],
            backgroundImage: "/images/1a_4a_classes.png",
            disciplines: [
                { name: "Língua Portuguesa", color: "#EF4444" },
                { name: "Matemática", color: "#3B82F6" },
                { name: "Meio Físico e Social", color: "#10B981" },
                { name: "Educação Física", color: "#F59E0B" },
                { name: "Educação Artística", color: "#8B5CF6" },
            ],
            badge: "FUNDAMENTOS",
            badgeColor: "from-blue-500/20 to-indigo-500/20 border-blue-400/40 text-blue-300",
            available: false,
        },
        {
            id: "base_c2",
            code: "base_c2",
            title: "5.ª e 6.ª Classes",
            subtitle: "Aceder ao 2.º Ciclo do Básico",
            description: "Ensino Básico - 2.º Ciclo. Expansão do conhecimento com 8 disciplinas diversificadas.",
            icon: "PiGlobeDuotone",
            iconBg: "bg-gradient-to-br from-cyan-500 via-teal-500 to-emerald-500",
            iconColor: "text-white",
            gradient: "from-cyan-500/10 via-teal-500/5 to-emerald-400/10",
            border: "border-cyan-400/40",
            buttonBg: "bg-gradient-to-r from-cyan-500 to-teal-500",
            buttonHover: "hover:from-cyan-600 hover:to-teal-600",
            accentColor: "text-cyan-400",
            overlayColor: "#CFFAFE",
            levels: ["5.ª Classe", "6.ª Classe"],
            backgroundImage: "/images/5a_6a_classes.png",
            disciplines: [
                { name: "Língua Portuguesa", color: "#EF4444" },
                { name: "Matemática", color: "#3B82F6" },
                { name: "Ciências da Natureza", color: "#10B981" },
                { name: "História", color: "#F472B6" },
                { name: "Geografia", color: "#06B6D4" },
                { name: "Educação Física", color: "#F59E0B" },
                { name: "Educação Artística", color: "#8B5CF6" },
                { name: "TIC", color: "#6366F1" },
            ],
            badge: "EXPLORAÇÃO",
            badgeColor: "from-cyan-500/20 to-teal-500/20 border-cyan-400/40 text-cyan-300",
            available: false,
        },
        {
            id: "base_c3",
            code: "base_c3",
            title: "7.ª a 9.ª Classes",
            subtitle: "Aceder ao 3.º Ciclo do Básico",
            description: "Ensino Básico - 3.º Ciclo. Preparação para o secundário com 11 disciplinas.",
            icon: "PiMicroscopeDuotone",
            iconBg: "bg-gradient-to-br from-violet-500 via-purple-600 to-indigo-600",
            iconColor: "text-white",
            gradient: "from-violet-500/10 via-purple-500/5 to-indigo-400/10",
            border: "border-violet-400/40",
            buttonBg: "bg-gradient-to-r from-violet-500 to-purple-600",
            buttonHover: "hover:from-violet-600 hover:to-purple-700",
            accentColor: "text-violet-400",
            overlayColor: "#E0E7FF",
            levels: ["7.ª Classe", "8.ª Classe", "9.ª Classe"],
            backgroundImage: "/images/7a_9a_classes.png",
            disciplines: [
                { name: "Português", color: "#EF4444" },
                { name: "Francês", color: "#7C3AED" },
                { name: "Inglês", color: "#2563EB" },
                { name: "Matemática", color: "#3B82F6" },
                { name: "Ciências Naturais", color: "#10B981" },
                { name: "Físico-Química", color: "#22D3EE" },
                { name: "História", color: "#F472B6" },
                { name: "Geografia", color: "#06B6D4" },
                { name: "TIC", color: "#6366F1" },
                { name: "Educação Física", color: "#F59E0B" },
                { name: "Educação Visual e Plástica", color: "#8B5CF6" },
            ],
            badge: "DESCOBERTA",
            badgeColor: "from-violet-500/20 to-purple-500/20 border-violet-400/40 text-violet-300",
            available: false,
        },
        {
            id: "seg_c1",
            code: "seg_c1",
            title: "10.ª a 12.ª Classes",
            subtitle: "Aceder ao Ensino Secundário",
            description: "Ensino Secundário - 1.º Ciclo. 4 especializações para o futuro profissional dos alunos.",
            icon: "RiGraduationCapLine",
            iconBg: "bg-gradient-to-br from-amber-400 via-yellow-500 to-orange-500",
            iconColor: "text-white",
            gradient: "from-amber-500/10 via-yellow-500/5 to-orange-400/10",
            border: "border-amber-400/40",
            buttonBg: "bg-gradient-to-r from-amber-500 to-orange-500",
            buttonHover: "hover:from-amber-600 hover:to-orange-600",
            accentColor: "text-amber-400",
            overlayColor: "#FEF3C7",
            levels: ["10.ª Classe", "11.ª Classe", "12.ª Classe"],
            specifications: [
                "Ciências e Tecnologias",
                "Ciências Humanas",
                "Economia e Gestão",
                "Artes Visuais",
            ],
            backgroundImage: "/images/secundario.png",
            disciplines: [
                { name: "Português", color: "#EF4444" },
                { name: "Francês", color: "#7C3AED" },
                { name: "Inglês", color: "#2563EB" },
                { name: "Matemática", color: "#3B82F6" },
                { name: "Física", color: "#22D3EE" },
                { name: "Química", color: "#14B8A6" },
                { name: "Biologia", color: "#10B981" },
                { name: "História", color: "#F472B6" },
                { name: "Geografia", color: "#06B6D4" },
                { name: "Filosofia", color: "#F97316" },
                { name: "Economia", color: "#84CC16" },
                { name: "Contabilidade", color: "#9333EA" },
            ],
            badge: "EXCELÊNCIA",
            badgeColor: "from-amber-500/20 to-orange-500/20 border-amber-400/40 text-amber-300",
            available: false,
        },
        {
            id: "technical",
            code: "technical",
            title: "Ensino Técnico",
            subtitle: "Aceder ao Ensino Técnico",
            description: "Formação profissional e técnica. Preparação para o mercado de trabalho são-tomense.",
            icon: "FiTool",
            iconBg: "bg-gradient-to-br from-slate-600 via-gray-700 to-zinc-800",
            iconColor: "text-white",
            gradient: "from-slate-500/10 via-gray-500/5 to-zinc-400/10",
            border: "border-slate-400/40",
            buttonBg: "bg-gradient-to-r from-slate-600 to-zinc-700",
            buttonHover: "hover:from-slate-700 hover:to-zinc-800",
            accentColor: "text-slate-300",
            overlayColor: "#F1F5F9",
            levels: ["Ensino Técnico"],
            backgroundImage: "/images/tecnico.png",
            disciplines: [],
            badge: "PROFISSIONAL",
            badgeColor: "from-slate-500/20 to-zinc-500/20 border-slate-400/40 text-slate-300",
            available: false,
        },
    ])

 
};

const res = await client.kv.call("kv.set",input);
console.log("resultado:", res); // { result: 5 }
} catch (error) {
  
}